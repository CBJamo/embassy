//! This example shows how to use `FlashPartition`

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::block::*;
use embassy_rp::flash::{Async, Flash, FlashPartition, FLASH_BASE};
use embassy_rp::peripherals;
use embassy_sync::{blocking_mutex::raw::NoopRawMutex, mutex::Mutex};
use embassy_time::Timer;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

const FLASH_SIZE: usize = 16 * 1024 * 1024;

#[link_section = ".start_block"]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

static FLASH: StaticCell<Mutex<NoopRawMutex, Flash<peripherals::FLASH, Async, FLASH_SIZE>>> = StaticCell::new();

unsafe extern "C" {
    static __start_block_addr: u32;
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    //
    // add some delay to give an attached debug probe time to parse the
    // defmt RTT header. Reading that header might touch flash memory, which
    // interferes with flash write operations.
    // https://github.com/knurling-rs/defmt/pull/683
    Timer::after_millis(100).await;

    // make a fake partition table, since we're not booted from one.
    let table = PartitionTableBlock::new().add_partition_item(
        UnpartitionedSpace::new().with_flag(UnpartitionedFlag::AcceptsDefaultFamilyAbsolute),
        &[
            Partition::new(0, 63)
                .with_id(0)
                .with_permission(Permission::SecureRead)
                .with_permission(Permission::SecureWrite)
                .with_name("Program"),
            Partition::new(64, 64)
                .with_id(1)
                .with_permission(Permission::SecureRead)
                .with_permission(Permission::SecureWrite)
                .with_name("Data"),
        ],
    );

    let prog = table.get_partition_by_name("Program").unwrap();
    let data = table.get_partition_by_name("Data").unwrap();

    let flash = embassy_rp::flash::Flash::new(p.FLASH, p.DMA_CH0);
    let flash_mutex = FLASH.init(Mutex::new(flash));
    let program_partition = FlashPartition::new(flash_mutex, prog);
    let data_partition = FlashPartition::new(flash_mutex, data);

    info!(
        "Program Partition capacity: {} kiB, {} sectors",
        program_partition.capacity() / 1024,
        program_partition.capacity() / 4096
    );
    info!(
        "Data Partition capacity: {} kiB, {} sectors",
        data_partition.capacity() / 1024,
        data_partition.capacity() / 4096
    );

    let program_offset = core::ptr::addr_of!(__start_block_addr) as u32 - FLASH_BASE as u32;
    const LEN: usize = 256;
    let mut buf = [0u8; LEN];
    program_partition.read(program_offset, &mut buf).await.unwrap();
    let mut words = [0u32; LEN / 4];
    for (i, chunk) in buf.chunks(4).enumerate() {
        words[i] = u32::from_le_bytes(chunk.try_into().unwrap());
    }
    info!(
        "First {} words of program memory. This should start with 0xFFFFDED3, the block start marker for our ImageDef.\n {:X}",
        words.len(),
        words
    );

    let mut buf = [0u8; 256];
    data_partition.read(0, &mut buf).await.unwrap();
    info!("Data partition before Erase {:X}", buf);

    data_partition.erase(0, 4096).await.unwrap();

    data_partition.read(0, &mut buf).await.unwrap();
    info!("Data partition after Erase {:X}", buf);

    for (i, byte) in buf.iter_mut().enumerate() {
        *byte = i as u8;
    }
    data_partition.write(0, &buf).await.unwrap();

    data_partition.read(0, &mut buf).await.unwrap();
    info!("After write {:X}", buf);
}
