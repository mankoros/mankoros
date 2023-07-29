pub mod address;
pub mod frame;
pub mod frame_ref_cnt;
pub mod heap;
pub mod pagetable;

mod user_ptr;

pub use address::kernel_phys_dev_to_virt;
pub use address::kernel_phys_to_virt;
pub use address::kernel_virt_text_to_phys;
pub use address::kernel_virt_to_phys;

pub type UserPtr<T, P> = user_ptr::UserPtr<T, P>;
pub type UserReadPtr<T> = user_ptr::UserReadPtr<T>;
pub type UserWritePtr<T> = user_ptr::UserWritePtr<T>;
pub type UserInOutPtr<T> = user_ptr::UserInOutPtr<T>;
