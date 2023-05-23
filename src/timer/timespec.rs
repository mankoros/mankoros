use core::ops::Add;

use super::NSEC_PER_SEC;

// copy from linux sys/times.h
// struct timespec {
//     time_t tv_sec;        /* seconds */
//     long   tv_nsec;       /* nanoseconds */
// };

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
pub struct TimeSpec {
    // seconds
    pub tv_sec: usize,
    // nanoseconds
    pub tv_nsec: usize,
}

impl TimeSpec {
    pub fn new(seconds: f64) -> Self {
        let tv_sec = seconds as usize;
        let tv_nsec = ((seconds - tv_sec as f64) * NSEC_PER_SEC as f64) as usize;
        Self { tv_sec, tv_nsec }
    }

    pub fn now() -> Self {
        let time = super::get_time_f64();
        Self::new(time)
    }

    /// 返回以秒为单位的时间
    pub fn time_in_sec(&self) -> f64 {
        self.tv_sec as f64 + self.tv_nsec as f64 / NSEC_PER_SEC as f64
    }
}

impl Add for TimeSpec {
    type Output = TimeSpec;
    fn add(self, other: Self) -> Self {
        let mut new_ts = Self {
            tv_sec: self.tv_sec + other.tv_sec,
            tv_nsec: self.tv_nsec + other.tv_nsec,
        };
        if new_ts.tv_nsec >= super::NSEC_PER_SEC {
            new_ts.tv_sec += 1;
            new_ts.tv_nsec -= super::NSEC_PER_SEC;
        }
        new_ts
    }
}
