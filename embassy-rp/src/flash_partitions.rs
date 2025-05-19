//! Flash driver.
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
#[cfg(feature = "rp235x-dfu")]
use embassy_usb::class::dfu;
use embedded_storage::nor_flash::{
    ErrorType, MultiwriteNorFlash, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash,
};
use embedded_storage_async::nor_flash::{
    MultiwriteNorFlash as AsyncMultiwriteNorFlash, NorFlash as AsyncNorFlash, ReadNorFlash as AsyncReadNorFlash,
};

use crate::block;
use crate::flash::*;

/// Error type for NVMC operations.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FlashPartitionError {
    /// Operation using a location not in flash.
    OutOfBounds,
    /// Unaligned operation or using unaligned buffers.
    Unaligned,
    /// Accessed from the wrong core.
    InvalidCore,
    /// Other error
    Other,
    /// Partition not writeable
    PermissionNotWriteable,
    /// Partition not readable
    PermissionNotReadable,
}

impl From<Error> for FlashPartitionError {
    fn from(e: Error) -> Self {
        match e {
            Error::Unaligned => Self::Unaligned,
            Error::OutOfBounds => Self::OutOfBounds,
            Error::InvalidCore => Self::InvalidCore,
            Error::Other => Self::Other,
        }
    }
}

impl From<NorFlashErrorKind> for FlashPartitionError {
    fn from(e: NorFlashErrorKind) -> Self {
        match e {
            NorFlashErrorKind::NotAligned => Self::Unaligned,
            NorFlashErrorKind::OutOfBounds => Self::OutOfBounds,
            _ => Self::Other,
        }
    }
}

impl NorFlashError for FlashPartitionError {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Self::OutOfBounds => NorFlashErrorKind::OutOfBounds,
            Self::Unaligned => NorFlashErrorKind::NotAligned,
            _ => NorFlashErrorKind::Other,
        }
    }
}

/// Access the flash addressed and permissioned by a partition.
/// Reads and writes to FlashPartitions are not cached by XIP.
pub struct FlashPartition<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> {
    flash: &'d Mutex<X, Flash<'d, T, Async, FLASH_SIZE>>,
    partition: block::Partition,
}

impl<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> FlashPartition<'d, X, T, FLASH_SIZE> {
    /// Create a new async PartitionFlash
    pub fn new(flash: &'d Mutex<X, Flash<'d, T, Async, FLASH_SIZE>>, partition: block::Partition) -> Self {
        Self { flash, partition }
    }

    /// Get the `Partition` for this `FlashPartition`
    pub fn partition(&self) -> block::Partition {
        self.partition.clone()
    }

    /// Get size in bytes of this partition.
    pub fn capacity(&self) -> usize {
        let (start, end) = self.partition.get_first_last_bytes();

        (end - start + 1) as usize
    }

    /// Read from the flash partition.
    ///
    /// The offset and buffer must be aligned.
    ///
    /// NOTE: `offset` is an offset from the partition start, NOT an absolute address.
    pub async fn read(&self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureRead) {
            return Err(FlashPartitionError::PermissionNotReadable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if offset as usize + bytes.len() > self.capacity() {
            return Err(FlashPartitionError::OutOfBounds);
        }

        let mut flash = self.flash.lock().await;
        flash.untranslated_blocking_read(start + offset, bytes)?;

        Ok(())
    }

    /// Erase from the flash partition. The actual erase is blocking, this function is async for the
    /// mutex.
    ///
    /// NOTE: `from` and `to` are offsets from the partition start, NOT an absolute address.
    pub async fn erase(&self, from: u32, to: u32) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureWrite) {
            return Err(FlashPartitionError::PermissionNotWriteable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if from > self.capacity() as u32 || to > self.capacity() as u32 {
            return Err(FlashPartitionError::OutOfBounds);
        }

        let mut flash = self.flash.lock().await;
        flash.untranslated_blocking_erase(start + from, start + to)?;

        Ok(())
    }

    /// Write to the flash partition. The actual write is blocking, this function is async for the
    /// mutex.
    ///
    /// NOTE: `offset` is an offset from the partition start, NOT an absolute address.
    pub async fn write(&self, offset: u32, bytes: &[u8]) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureWrite) {
            return Err(FlashPartitionError::PermissionNotWriteable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if offset as usize + bytes.len() > self.capacity() {
            return Err(FlashPartitionError::OutOfBounds);
        }

        let mut flash = self.flash.lock().await;
        flash.untranslated_blocking_write(start + offset, bytes)?;

        Ok(())
    }
}

impl<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> ErrorType for FlashPartition<'d, X, T, FLASH_SIZE> {
    type Error = FlashPartitionError;
}

impl<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> AsyncReadNorFlash for FlashPartition<'d, X, T, FLASH_SIZE> {
    const READ_SIZE: usize = READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        FlashPartition::read(self, offset, bytes).await
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }
}

impl<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> AsyncMultiwriteNorFlash
    for FlashPartition<'d, X, T, FLASH_SIZE>
{
}

impl<'d, X: RawMutex, T: Instance, const FLASH_SIZE: usize> AsyncNorFlash for FlashPartition<'d, X, T, FLASH_SIZE> {
    const WRITE_SIZE: usize = WRITE_SIZE;

    const ERASE_SIZE: usize = ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        FlashPartition::erase(self, from, to).await
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        FlashPartition::write(self, offset, bytes).await
    }
}

/// Access the flash addressed and permissioned by a partition.
/// Reads and writes to FlashPartitions are not cached by XIP.
pub struct ExclusiveFlashPartition<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> {
    flash: Flash<'d, T, M, FLASH_SIZE>,
    partition: block::Partition,
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> ExclusiveFlashPartition<'d, T, M, FLASH_SIZE> {
    /// Create a new async ExclusiveFlashPartition.
    /// ExclusiveFlashPartition can be made with async or blocking mode Flash, but all ops are
    /// blocking.
    pub fn new(flash: Flash<'d, T, M, FLASH_SIZE>, partition: block::Partition) -> Self {
        Self { flash, partition }
    }

    /// Get the `Partition` for this `FlashPartition`
    pub fn partition(&self) -> block::Partition {
        self.partition.clone()
    }

    /// Get size in bytes of this partition.
    pub fn capacity(&self) -> usize {
        let (start, end) = self.partition.get_first_last_bytes();

        (end - start + 1) as usize
    }

    /// Blocking read from the flash partition.
    ///
    /// The offset and buffer must be aligned.
    ///
    /// NOTE: `offset` is an offset from the partition start, NOT an absolute address.
    pub fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureRead) {
            return Err(FlashPartitionError::PermissionNotReadable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if offset as usize + bytes.len() > self.capacity() {
            return Err(FlashPartitionError::OutOfBounds);
        }

        self.flash.untranslated_blocking_read(start + offset, bytes)?;

        Ok(())
    }

    /// Blocking erase to the flash partition.
    ///
    /// NOTE: `from` and `to` are offsets from the partition start, NOT an absolute address.
    pub fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureWrite) {
            return Err(FlashPartitionError::PermissionNotWriteable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if from > self.capacity() as u32 || to > self.capacity() as u32 {
            return Err(FlashPartitionError::OutOfBounds);
        }

        self.flash.untranslated_blocking_erase(start + from, start + to)?;

        Ok(())
    }

    /// Blocking write to the flash partition.
    ///
    /// NOTE: `offset` is an offset from the partition start, NOT an absolute address.
    pub fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashPartitionError> {
        // TODO: Handle nonsecure
        if !self.partition.has_permission(crate::block::Permission::SecureWrite) {
            return Err(FlashPartitionError::PermissionNotWriteable);
        }

        let (start, _) = self.partition.get_first_last_bytes();

        if offset as usize + bytes.len() > self.capacity() {
            return Err(FlashPartitionError::OutOfBounds);
        }

        self.flash.untranslated_blocking_write(start + offset, bytes)?;

        Ok(())
    }
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> ErrorType for ExclusiveFlashPartition<'d, T, M, FLASH_SIZE> {
    type Error = FlashPartitionError;
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> ReadNorFlash for ExclusiveFlashPartition<'d, T, M, FLASH_SIZE> {
    const READ_SIZE: usize = READ_SIZE;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::read(self, offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> MultiwriteNorFlash
    for ExclusiveFlashPartition<'d, T, M, FLASH_SIZE>
{
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> NorFlash for ExclusiveFlashPartition<'d, T, M, FLASH_SIZE> {
    const WRITE_SIZE: usize = WRITE_SIZE;

    const ERASE_SIZE: usize = ERASE_SIZE;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::erase(self, from, to)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::write(self, offset, bytes)
    }
}

impl<'d, T: Instance, const FLASH_SIZE: usize> AsyncReadNorFlash for ExclusiveFlashPartition<'d, T, Async, FLASH_SIZE> {
    const READ_SIZE: usize = READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::read(self, offset, bytes)
    }

    fn capacity(&self) -> usize {
        self.capacity()
    }
}

impl<'d, T: Instance, const FLASH_SIZE: usize> AsyncMultiwriteNorFlash
    for ExclusiveFlashPartition<'d, T, Async, FLASH_SIZE>
{
}

impl<'d, T: Instance, const FLASH_SIZE: usize> AsyncNorFlash for ExclusiveFlashPartition<'d, T, Async, FLASH_SIZE> {
    const WRITE_SIZE: usize = WRITE_SIZE;

    const ERASE_SIZE: usize = ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::erase(self, from, to)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        ExclusiveFlashPartition::write(self, offset, bytes)
    }
}

/// Errors when creating an InactivePartition
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum InactivePartitionCreationError {
    /// Couldn't find a partition table in the search area
    NoPartitionTableFound,
    /// There was not an inactive partition to select
    NoInactivePartition,
}

/// A flash partition that can only be constructed if there is an inactive A/B program parition.
/// If the rp235x-dfu feature is enabled, this implments the embassy_usb::dfu::dfu_mode::Handler trait.
pub struct InactivePartition<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> {
    partition: ExclusiveFlashPartition<'d, T, M, FLASH_SIZE>,
    last_erased_page: u32,
    last_written_byte: u32,
}

impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> InactivePartition<'d, T, M, FLASH_SIZE> {
    /// Search for a partition table starting at search_start and looking for up to search_length
    /// bytes. search_start is an offset from the start of flash.
    pub fn new(
        mut flash: Flash<'d, T, M, FLASH_SIZE>,
        search_start: u32,
        search_length: u32,
    ) -> Result<Self, InactivePartitionCreationError> {
        if let Some(pt_start) = block::find_block_start(
            &mut flash,
            search_start,
            search_length,
            Some(block::ITEM_2BS_PARTITION_TABLE),
        )
        .expect("We're executing from this flash, errors should not be possible.")
        {
            let pt = block::PartitionTableBlock::from_flash(&mut flash, pt_start).unwrap();
            if let Some(partition) = pt.get_inactive_partition() {
                debug!(
                    "Found OTA partition \"{}\" at 0x{:X}",
                    partition.get_name(),
                    partition.get_first_last_bytes()
                );

                let partition = ExclusiveFlashPartition::new(flash, partition);

                Ok(Self {
                    partition,
                    last_erased_page: 0,
                    last_written_byte: 0,
                })
            } else {
                Err(InactivePartitionCreationError::NoInactivePartition)
            }
        } else {
            warn!("No OTA partition found");

            Err(InactivePartitionCreationError::NoPartitionTableFound)
        }
    }

    /// Get the `Partition` for this `FlashPartition`
    pub fn partition(&'d mut self) -> &'d mut ExclusiveFlashPartition<'d, T, M, FLASH_SIZE> {
        &mut self.partition
    }
}

#[cfg(feature = "rp235x-dfu")]
impl<'d, T: Instance, M: Mode, const FLASH_SIZE: usize> dfu::dfu_mode::Handler
    for InactivePartition<'d, T, M, FLASH_SIZE>
{
    fn start(&mut self) {
        debug!("Starting DFU update");
        self.last_erased_page = 0;
        self.last_written_byte = 0;
    }

    fn write(&mut self, data: &[u8]) -> Result<(), dfu::consts::Status> {
        let data_len = data.len() as u32;

        let last_erased_byte = self.last_erased_page * 4096;
        if last_erased_byte < self.last_written_byte + data_len {
            let erase_pages = data_len.div_ceil(4096);
            if self
                .partition
                .erase(last_erased_byte, last_erased_byte + erase_pages * 4096)
                .is_err()
            {
                return Err(dfu::consts::Status::ErrProg);
            }

            debug!(
                "Erasing bytes {}-{}",
                last_erased_byte,
                last_erased_byte + erase_pages * 4096
            );

            for i in 0..erase_pages {
                let mut check_buf = [0u8; 4096];
                if self
                    .partition
                    .read(last_erased_byte + i * 4096, &mut check_buf)
                    .is_err()
                {
                    return Err(dfu::consts::Status::ErrProg);
                }
                if check_buf.iter().any(|x| *x != 0xFF) {
                    return Err(dfu::consts::Status::ErrCheckErased);
                }
            }

            self.last_erased_page += erase_pages;
        }

        debug!(
            "Writing bytes {}-{}",
            self.last_written_byte,
            self.last_written_byte + data_len
        );
        if self.partition.write(self.last_written_byte, data).is_err() {
            return Err(dfu::consts::Status::ErrProg);
        }

        let mut last_checked = self.last_written_byte;
        loop {
            debug!("Checking bytes {}-{}", last_checked, last_checked + data_len);

            let mut check_chunk = [0u8; 4096];
            if self.partition.read(last_checked, &mut check_chunk).is_err() {
                return Err(dfu::consts::Status::ErrProg);
            }
            last_checked += check_chunk.len() as u32;

            for (d_byte, c_byte) in data.iter().zip(check_chunk) {
                if *d_byte != c_byte {
                    return Err(dfu::consts::Status::ErrVerify);
                }
            }

            if last_checked >= self.last_written_byte {
                break;
            }
        }

        self.last_written_byte += data_len;

        Ok(())
    }

    fn finish(&mut self) -> Result<(), embassy_usb::class::dfu::consts::Status> {
        // TODO: Search new image for a hash (and signature?), and if it has one, verify it.
        Ok(())
    }

    fn system_reset(&mut self) {
        let (inactive_partition_addr, _) = self.partition.partition().get_first_last_bytes();

        let start_addr = (inactive_partition_addr + FLASH_BASE as u32) as *const u32;

        info!(
            "Rebooting to partition {} at {}",
            self.partition.partition().get_name(),
            start_addr
        );

        crate::reboot::reboot(crate::reboot::RebootKind::FlashUpdate { start_addr });
    }
}
