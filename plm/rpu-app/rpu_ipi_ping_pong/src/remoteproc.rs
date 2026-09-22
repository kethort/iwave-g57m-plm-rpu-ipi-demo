#![allow(dead_code)]

#[repr(C)]
pub struct ResourceTable {
    ver: u32,
    num: u32,
    reserved: [u32; 2],
    offset: [u32; 1],
}

// Minimal remoteproc resource table. The firmware does not reserve vrings or
// carveouts, but the table must exist so Linux remoteproc can recognize the ELF.
#[used]
#[link_section = ".resource_table"]
pub static RESOURCE_TABLE: ResourceTable = ResourceTable {
    ver: 1,
    num: 0,
    reserved: [0; 2],
    offset: [0; 1],
};

#[inline(never)]
pub(crate) fn resource_table_address() -> usize {
    core::ptr::addr_of!(RESOURCE_TABLE) as usize
}
