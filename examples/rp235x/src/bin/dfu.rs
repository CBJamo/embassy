//! This example shows how to use an `InactivePartition` with usb dfu. You must have a partition
//! table in the first page of flash for this example to function.
//!
//! Note that if you're using OTA like this, you probably want to use the `imagedef-none` feature
//! and supply a `ImageDevVersion` to make updates more controlled. If neither image has a
//! versioned image def, the bootrom will boot into the last image it finds.

#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::flash::Flash;
use embassy_rp::flash_partitions::InactivePartition;
use embassy_rp::peripherals;
use embassy_rp::usb::Driver as UsbDriver;
use embassy_usb::{Builder, Config, class::dfu};
use {defmt_rtt as _, panic_probe as _};

const FLASH_SIZE: usize = 16 * 1024 * 1024;

bind_interrupts!(struct Irqs {
    DMA_IRQ_0 => embassy_rp::dma::InterruptHandler<peripherals::DMA_CH0>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<peripherals::USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let driver = UsbDriver::new(p.USB, Irqs);
    let flash: embassy_rp::flash::Flash<'_, _, _, FLASH_SIZE> = Flash::new(p.FLASH, p.DMA_CH0, Irqs);
    // Search the first page of flash for a partition table and make an InactivePartition.
    let inactive_partition = InactivePartition::new(flash, 0, 4096).unwrap();

    // Create embassy-usb Config
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Embassy");
    config.product = Some("Partition DFU example");
    config.serial_number = Some("12345678");

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 4096];

    let dfu_attrs = dfu::consts::DfuAttributes::CAN_DOWNLOAD;
    let mut dfu_state = dfu::dfu_mode::DfuState::new(inactive_partition, dfu_attrs);

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    dfu::dfu_mode::usb_dfu(&mut builder, &mut dfu_state, 4096, |_| {});

    let mut usb = builder.build();

    info!("Waiting for USB DFU image.");
    // Run the USB device. At this point we'll wait forever for a host to give us an update, then
    // reboot to that new firmware.
    usb.run().await;
}
