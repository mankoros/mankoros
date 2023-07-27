//  struct rusage {
//    struct timeval ru_utime; /* user CPU time used */
//    struct timeval ru_stime; /* system CPU time used */
//    ...
//  };
use super::TimeVal;

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct Rusage {
    // user CPU time used
    pub ru_utime: TimeVal,
    // system CPU time used
    pub ru_stime: TimeVal,
}