// copy from linux sys/times.h
// struct tms {
//     clock_t tms_utime;  /* user time */
//     clock_t tms_stime;  /* system time */
//     clock_t tms_cutime; /* user time of children */
//     clock_t tms_cstime; /* system time of children */
// };

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Tms {
    // user time
    pub tms_utime: usize,
    // system time
    pub tms_stime: usize,
    // user time of children
    pub tms_cutime: usize,
    // system time of children
    pub tms_cstime: usize,
}
