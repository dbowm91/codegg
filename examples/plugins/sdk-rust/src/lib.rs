pub mod abi;
pub mod builders;

pub use codegg_protocol;

#[macro_export]
macro_rules! codegg_plugin {
    ($handler:expr) => {
        #[no_mangle]
        pub extern "C" fn allocate(len: i32) -> i32 {
            $crate::abi::allocate(len)
        }

        #[no_mangle]
        pub extern "C" fn deallocate(ptr: i32, len: i32) {
            $crate::abi::deallocate(ptr, len)
        }

        #[no_mangle]
        pub extern "C" fn codegg_plugin_invoke(ptr: i32, len: i32) -> i64 {
            $crate::abi::do_invoke(ptr, len, $handler)
        }
    };
}
