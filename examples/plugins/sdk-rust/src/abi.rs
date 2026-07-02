use codegg_protocol::plugin::{PluginInvocation, PluginResponse};
use core::ptr::addr_of_mut;

const HEAP_SIZE: usize = 1024 * 1024;

static mut HEAP: [u8; HEAP_SIZE] = [0u8; HEAP_SIZE];
static mut HEAP_OFFSET: usize = 0;

pub fn allocate(len: i32) -> i32 {
    let len = len as usize;
    unsafe {
        let ptr = HEAP_OFFSET;
        if ptr + len > HEAP_SIZE {
            panic!("out of wasm memory");
        }
        HEAP_OFFSET += len;
        addr_of_mut!(HEAP).cast::<u8>().add(ptr) as i32
    }
}

pub fn deallocate(_ptr: i32, _len: i32) {}

pub fn do_invoke(ptr: i32, len: i32, handler: fn(PluginInvocation) -> PluginResponse) -> i64 {
    unsafe {
        let input_bytes = core::slice::from_raw_parts(ptr as *const u8, len as usize);
        let invocation: PluginInvocation =
            serde_json::from_slice(input_bytes).expect("failed to deserialize PluginInvocation");
        let response = handler(invocation);
        let response_json =
            serde_json::to_vec(&response).expect("failed to serialize PluginResponse");
        let resp_len = response_json.len();
        let resp_ptr = allocate(resp_len as i32);
        core::ptr::copy_nonoverlapping(response_json.as_ptr(), resp_ptr as *mut u8, resp_len);
        ((resp_ptr as i64) << 32) | (resp_len as i64)
    }
}
