// SPDX-License-Identifier: MIT
// SigmaOS-gpart - Partition Table Editor component
// gpart-style partition table manipulation for SigmaOS,
// inspired by Arch's gptfdisk/parted-style partitioning tools.

use std::fmt;
use std::path::PathBuf;

// ============================================================================
// PARTITION TABLE TYPES
// ============================================================================

/// Supported partition table formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableType {
    /// GUID Partition Table (UEFI)
    Gpt,
    /// Master Boot Record (legacy BIOS)
    Mbr,
}

impl TableType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gpt => "gpt",
            Self::Mbr => "mbr",
        }
    }
}

/// Filesystem type codes (MBR type bytes / GPT type GUIDs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    Empty,
    Fat16,
    Fat32,
    Ntfs,
    Ext2,
    Ext3,
    Ext4,
    LinuxSwap,
    LinuxLvm,
    LinuxRaid,
    Xfs,
    Btrfs,
    GptEfiSystem,
    GptBiosBoot,
    GptLinuxRoot,
    GptLinuxHome,
    GptLinuxSwap,
    GptLinuxLvm,
    GptLinuxRaid,
    Unknown(u8),
}

impl FsType {
    /// MBR partition type byte
    pub fn mbr_type(&self) -> u8 {
        match self {
            Self::Empty => 0x00,
            Self::Fat16 => 0x06,
            Self::Fat32 => 0x0C,
            Self::Ntfs => 0x07,
            Self::Ext2 | Self::Ext3 | Self::Ext4 => 0x83,
            Self::LinuxSwap => 0x82,
            Self::LinuxLvm => 0x8E,
            Self::LinuxRaid => 0xFD,
            Self::Xfs | Self::Btrfs => 0x83,
            _ => 0x00,
        }
    }

    /// GPT partition type GUID (first 8 hex chars of GUID)
    pub fn gpt_type_guid(&self) -> &'static str {
        match self {
            Self::GptEfiSystem => "C12A7328",
            Self::GptBiosBoot => "21686148",
            Self::GptLinuxRoot => "4F68BCE3",
            Self::GptLinuxHome => "933AC7E1",
            Self::GptLinuxSwap => "0657FD6D",
            Self::GptLinuxLvm => "E6D6D379",
            Self::GptLinuxRaid => "A19D880F",
            _ => "0FC63DAF", // Linux filesystem data
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Fat16 => "fat16",
            Self::Fat32 => "fat32",
            Self::Ntfs => "ntfs",
            Self::Ext2 => "ext2",
            Self::Ext3 => "ext3",
            Self::Ext4 => "ext4",
            Self::LinuxSwap => "linux-swap",
            Self::LinuxLvm => "linux-lvm",
            Self::LinuxRaid => "linux-raid",
            Self::Xfs => "xfs",
            Self::Btrfs => "btrfs",
            _ => "unknown",
        }
    }
}

// ============================================================================
// PARTITION ENTRY
// ============================================================================

/// A single partition entry
#[derive(Debug, Clone)]
pub struct Partition {
    /// Partition number (1-based)
    pub number: u8,
    /// Start sector (LBA)
    pub start_lba: u64,
    /// End sector (LBA, inclusive)
    pub end_lba: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Filesystem type
    pub fs_type: FsType,
    /// Partition name (GPT only)
    pub name: String,
    /// Whether partition is bootable/active (MBR flag)
    pub bootable: bool,
    /// Whether partition is hidden
    pub hidden: bool,
    /// GUID (GPT only)
    pub guid: Option<String>,
    /// Unique partition GUID (GPT only)
    pub unique_guid: Option<String>,
    /// First usable LBA (GPT alignment)
    pub alignment: u64,
}

impl Partition {
    /// Number of sectors
    pub fn sectors(&self) -> u64 {
        self.end_lba.saturating_sub(self.start_lba) + 1
    }

    /// Human-readable size
    pub fn size_human(&self, sector_size: u64) -> String {
        let bytes = self.sectors() * sector_size;
        format_size(bytes)
    }

    /// Whether this partition overlaps another
    pub fn overlaps(&self, other: &Partition) -> bool {
        !(self.end_lba < other.start_lba || self.start_lba > other.end_lba)
    }
}

/// A partition table (MBR or GPT)
#[derive(Debug, Clone)]
pub struct PartitionTable {
    /// Table type
    pub table_type: TableType,
    /// Sector size in bytes
    pub sector_size: u64,
    /// Total sectors on device
    pub total_sectors: u64,
    /// Total bytes on device
    pub total_bytes: u64,
    /// Partitions
    pub partitions: Vec<Partition>,
    /// Disk signature (MBR) or GUID (GPT)
    pub signature: String,
    /// Whether table is valid
    pub valid: bool,
}

impl PartitionTable {
    pub fn new(table_type: TableType, total_bytes: u64) -> Self {
        let sector_size = 512u64;
        Self {
            table_type,
            sector_size,
            total_sectors: total_bytes / sector_size,
            total_bytes,
            partitions: Vec::new(),
            signature: String::new(),
            valid: true,
        }
    }

    /// Create a standard layout: EFI + root + home + swap
    pub fn standard_layout(&mut self) {
        let disk_gb = self.total_bytes / (1024 * 1024 * 1024);
        let mut lba = match self.table_type {
            TableType::Gpt => 34, // GPT header + entries
            TableType::Mbr => 2048, // 1MiB alignment
        };
        let alignment = 2048u64; // 1MiB

        // EFI system partition (512MiB)
        let efi_size = 512 * 1024 * 1024 / self.sector_size;
        self.add_partition(FsType::GptEfiSystem, lba, efi_size, "EFI", true);
        lba = self.partitions.last().map(|p| p.end_lba + 1).unwrap_or(lba);

        // Root partition (30% or 50GB min)
        let root_size = if disk_gb > 200 {
            100 * 1024 * 1024 * 1024 / self.sector_size // 100GB
        } else {
            self.total_sectors / 3
        };
        self.add_partition(FsType::GptLinuxRoot, lba, root_size, "root", false);
        lba = self.partitions.last().map(|p| p.end_lba + 1).unwrap_or(lba);

        // Home partition (remaining minus swap)
        let swap_size = if self.total_bytes > 8 * 1024 * 1024 * 1024 {
            8 * 1024 * 1024 * 1024 / self.sector_size // 8GB swap
        } else {
            2 * 1024 * 1024 * 1024 / self.sector_size
        };
        let home_size = self.total_sectors.saturating_sub(lba).saturating_sub(swap_size);
        if home_size > alignment {
            self.add_partition(FsType::GptLinuxHome, lba, home_size, "home", false);
            lba = self.partitions.last().map(|p| p.end_lba + 1).unwrap_or(lba);
        }

        // Swap partition (remaining)
        let remaining = self.total_sectors.saturating_sub(lba);
        if remaining > alignment {
            self.add_partition(FsType::GptLinuxSwap, lba, remaining, "swap", false);
        }
    }

    fn add_partition(&mut self, fs_type: FsType, start_lba: u64, size_sectors: u64, name: &str, bootable: bool) {
        // Align start
        let align = 2048u64;
        let aligned_start = (start_lba + align - 1) / align * align;
        let aligned_end = aligned_start + size_sectors;
        if aligned_end > self.total_sectors {
            return; // doesn't fit
        }
        let number = self.partitions.len() as u8 + 1;
        self.partitions.push(Partition {
            number,
            start_lba: aligned_start,
            end_lba: aligned_end.saturating_sub(1),
            size_bytes: size_sectors * self.sector_size,
            fs_type,
            name: name.to_string(),
            bootable,
            hidden: false,
            guid: None,
            unique_guid: None,
            alignment: align,
        });
    }

    /// Find partition by number
    pub fn get(&self, number: u8) -> Option<&Partition> {
        self.partitions.iter().find(|p| p.number == number)
    }

    /// Find partition by name
    pub fn get_by_name(&self, name: &str) -> Option<&Partition> {
        self.partitions.iter().find(|p| p.name == name)
    }

    /// Check for overlapping partitions
    pub fn has_overlaps(&self) -> bool {
        for i in 0..self.partitions.len() {
            for j in (i + 1)..self.partitions.len() {
                if self.partitions[i].overlaps(&self.partitions[j]) {
                    return true;
                }
            }
        }
        false
    }

    /// Total allocated space
    pub fn used_bytes(&self) -> u64 {
        self.partitions.iter().map(|p| p.size_bytes).sum()
    }

    /// Free space on disk
    pub fn free_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.used_bytes())
    }

    /// Validate partition table consistency
    pub fn validate(&self) -> Result<(), PartitionError> {
        if self.table_type == TableType::Gpt && self.partitions.len() > 128 {
            return Err(PartitionError::TooManyPartitions);
        }
        if self.table_type == TableType::Mbr && self.partitions.len() > 4 {
            return Err(PartitionError::TooManyPrimaryPartitions);
        }
        if self.has_overlaps() {
            return Err(PartitionError::OverlappingPartitions);
        }
        // Check partitions fit within disk
        for p in &self.partitions {
            if p.end_lba >= self.total_sectors {
                return Err(PartitionError::PartitionExceedsDisk(p.number));
            }
        }
        Ok(())
    }
}

// ============================================================================
// PARTITION OPERATIONS
// ============================================================================

/// Result of a partition operation
#[derive(Debug, Clone)]
pub struct OperationResult {
    pub success: bool,
    pub message: String,
    pub backup_path: Option<PathBuf>,
}

/// Partition manager — applies changes to a table
pub struct PartitionManager {
    table: PartitionTable,
    /// Whether changes have been applied
    modified: bool,
}

impl PartitionManager {
    pub fn new(table: PartitionTable) -> Self {
        Self { table, modified: false }
    }

    pub fn table(&self) -> &PartitionTable { &self.table }
    pub fn is_modified(&self) -> bool { self.modified }

    /// Create a new partition
    pub fn create(&mut self, fs_type: FsType, start_lba: u64, size_sectors: u64, name: &str) -> Result<u8, PartitionError> {
        let new_part = Partition {
            number: (self.table.partitions.len() + 1) as u8,
            start_lba,
            end_lba: start_lba + size_sectors - 1,
            size_bytes: size_sectors * self.table.sector_size,
            fs_type,
            name: name.to_string(),
            bootable: false,
            hidden: false,
            guid: None,
            unique_guid: None,
            alignment: 2048,
        };

        // Check for overlap
        for p in &self.table.partitions {
            if new_part.overlaps(p) {
                return Err(PartitionError::OverlappingPartitions);
            }
        }

        // Check fits
        if new_part.end_lba >= self.table.total_sectors {
            return Err(PartitionError::PartitionExceedsDisk(new_part.number));
        }

        let num = new_part.number;
        self.table.partitions.push(new_part);
        self.modified = true;
        Ok(num)
    }

    /// Delete a partition by number
    pub fn delete(&mut self, number: u8) -> Result<(), PartitionError> {
        let idx = self.table.partitions.iter()
            .position(|p| p.number == number)
            .ok_or(PartitionError::PartitionNotFound(number))?;
        self.table.partitions.remove(idx);
        // Renumber remaining partitions
        for (i, p) in self.table.partitions.iter_mut().enumerate() {
            p.number = (i + 1) as u8;
        }
        self.modified = true;
        Ok(())
    }

    /// Resize a partition
    pub fn resize(&mut self, number: u8, new_size_sectors: u64) -> Result<(), PartitionError> {
        let idx = self.table.partitions.iter()
            .position(|p| p.number == number)
            .ok_or(PartitionError::PartitionNotFound(number))?;
        let start = self.table.partitions[idx].start_lba;
        let new_end = start + new_size_sectors - 1;
        if new_end >= self.table.total_sectors {
            return Err(PartitionError::PartitionExceedsDisk(number));
        }
        // Check doesn't overlap other partitions
        for (j, other) in self.table.partitions.iter().enumerate() {
            if j != idx && !(new_end < other.start_lba || start > other.end_lba) {
                return Err(PartitionError::OverlappingPartitions);
            }
        }
        let part = &mut self.table.partitions[idx];
        part.end_lba = new_end;
        part.size_bytes = new_size_sectors * self.table.sector_size;
        self.modified = true;
        Ok(())
    }

    /// Set filesystem type
    pub fn set_fs_type(&mut self, number: u8, fs_type: FsType) -> Result<(), PartitionError> {
        let part = self.table.partitions.iter_mut()
            .find(|p| p.number == number)
            .ok_or(PartitionError::PartitionNotFound(number))?;
        part.fs_type = fs_type;
        self.modified = true;
        Ok(())
    }

    /// Set bootable flag
    pub fn set_bootable(&mut self, number: u8, bootable: bool) -> Result<(), PartitionError> {
        let part = self.table.partitions.iter_mut()
            .find(|p| p.number == number)
            .ok_or(PartitionError::PartitionNotFound(number))?;
        part.bootable = bootable;
        self.modified = true;
        Ok(())
    }

    /// Apply changes (write to disk — simulated)
    pub fn apply(&self) -> OperationResult {
        if self.table.validate().is_err() {
            return OperationResult {
                success: false,
                message: "partition table validation failed".into(),
                backup_path: None,
            };
        }
        OperationResult {
            success: true,
            message: format!("{} partitions written", self.table.partitions.len()),
            backup_path: Some(PathBuf::from("/tmp/gpt-backup.bin")),
        }
    }
}

// ============================================================================
// DISPLAY
// ============================================================================

/// Format partition table for display (fdisk -l style)
pub fn print_table(table: &PartitionTable) -> String {
    let mut out = String::new();
    out.push_str(&format!("Disk: {} bytes, {} sectors\n", table.total_bytes, table.total_sectors));
    out.push_str(&format!("Table type: {}\n", table.table_type.as_str()));
    out.push_str(&format!("Sector size: {} bytes\n\n", table.sector_size));
    out.push_str(&format!("{:>4} {:>12} {:>12} {:>10} {:>8} {:>8} {}\n",
        "Num", "Start (LBA)", "End (LBA)", "Sectors", "Size", "Type", "Name"));
    out.push_str(&"---- ------------ ------------ ---------- -------- -------- ----\n");
    for p in &table.partitions {
        out.push_str(&format!("{:>4} {:>12} {:>12} {:>10} {:>8} {:>8} {}\n",
            p.number,
            p.start_lba,
            p.end_lba,
            p.sectors(),
            p.size_human(table.sector_size),
            p.fs_type.name(),
            p.name));
    }
    out
}

// ============================================================================
// ERRORS
// ============================================================================

#[derive(Debug)]
pub enum PartitionError {
    TooManyPartitions,
    TooManyPrimaryPartitions,
    OverlappingPartitions,
    PartitionNotFound(u8),
    PartitionExceedsDisk(u8),
    InvalidTable,
    PermissionDenied,
}

impl fmt::Display for PartitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPartitions => write!(f, "too many partitions (max 128)"),
            Self::TooManyPrimaryPartitions => write!(f, "too many primary partitions (max 4 for MBR)"),
            Self::OverlappingPartitions => write!(f, "overlapping partitions"),
            Self::PartitionNotFound(n) => write!(f, "partition not found: {}", n),
            Self::PartitionExceedsDisk(n) => write!(f, "partition {} exceeds disk size", n),
            Self::InvalidTable => write!(f, "invalid partition table"),
            Self::PermissionDenied => write!(f, "permission denied"),
        }
    }
}
impl std::error::Error for PartitionError {}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} B", bytes) } else { format!("{:.1} {}", size, UNITS[unit]) }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_layout() {
        let mut table = PartitionTable::new(TableType::Gpt, 500 * 1024 * 1024 * 1024);
        table.standard_layout();
        assert!(table.partitions.len() >= 3);
        assert!(table.validate().is_ok());
        // EFI should be first and bootable
        assert!(table.partitions[0].bootable);
        assert_eq!(table.partitions[0].fs_type, FsType::GptEfiSystem);
    }

    #[test]
    fn test_create_delete_partition() {
        let table = PartitionTable::new(TableType::Gpt, 100 * 1024 * 1024 * 1024);
        let mut pm = PartitionManager::new(table);

        let num = pm.create(FsType::Ext4, 2048, 10000, "test").unwrap();
        assert_eq!(num, pm.table().partitions.len() as u8);
        pm.delete(num).unwrap();
        assert!(pm.table().get(num).is_none());
    }

    #[test]
    fn test_overlap_detection() {
        let mut table = PartitionTable::new(TableType::Gpt, 100 * 1024 * 1024 * 1024);
        table.standard_layout();
        let mut pm = PartitionManager::new(table);

        // Try to create overlapping partition
        let existing = pm.table().partitions[0].clone();
        let result = pm.create(FsType::Ext4, existing.start_lba, 100, "overlap");
        assert!(result.is_err());
    }

    #[test]
    fn test_resize_partition() {
        let table = PartitionTable::new(TableType::Gpt, 100 * 1024 * 1024 * 1024);
        let mut pm = PartitionManager::new(table);
        pm.create(FsType::Ext4, 2048, 50000, "data").unwrap();

        pm.resize(1, 25000).unwrap();
        assert!(pm.table().get(1).unwrap().size_bytes < 50000 * 512);
    }

    #[test]
    fn test_bootable_flag() {
        let mut table = PartitionTable::new(TableType::Mbr, 500 * 1024 * 1024 * 1024);
        table.standard_layout();
        let mut pm = PartitionManager::new(table);
        pm.set_bootable(1, true).unwrap();
        assert!(pm.table().partitions[0].bootable);
    }

    #[test]
    fn test_print_table() {
        let mut table = PartitionTable::new(TableType::Gpt, 100 * 1024 * 1024 * 1024);
        table.standard_layout();
        let out = print_table(&table);
        assert!(out.contains("Num"));
        assert!(out.contains("EFI"));
        assert!(out.contains("root"));
    }

    #[test]
    fn test_fs_type_mbr_gpt() {
        assert_eq!(FsType::Ext4.mbr_type(), 0x83);
        assert_eq!(FsType::LinuxSwap.mbr_type(), 0x82);
        assert_eq!(FsType::GptEfiSystem.gpt_type_guid(), "C12A7328");
    }

    #[test]
    fn test_free_space() {
        let mut table = PartitionTable::new(TableType::Gpt, 100 * 1024 * 1024 * 1024);
        table.standard_layout();
        assert!(table.free_bytes() > 0);
        assert!(table.used_bytes() > 0);
        assert!(table.used_bytes() + table.free_bytes() <= table.total_bytes);
    }
}
