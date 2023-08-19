use core::ops::Add;

use crate::consts;

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

const E3: usize = 1000;
const E6: usize = 1000000;
const E9: usize = 1000000000;

impl TimeSpec {
    pub fn new(sec: usize, nsec: usize) -> Self {
        Self {
            tv_sec: sec,
            tv_nsec: nsec,
        }
    }

    pub fn now() -> Self {
        let now_us = super::get_time_us();
        let tv_sec = now_us / E6;
        let tv_nsec = (now_us % E6) * E3;
        Self { tv_sec, tv_nsec }
    }

    pub fn time_in_ms(&self) -> usize {
        self.tv_sec * E3 + self.tv_nsec / E6
    }

    pub fn time_in_ns(&self) -> usize {
        self.tv_sec * E9 + self.tv_nsec
    }
}

impl Add for TimeSpec {
    type Output = TimeSpec;
    fn add(self, other: Self) -> Self {
        let mut new_ts = Self {
            tv_sec: self.tv_sec + other.tv_sec,
            tv_nsec: self.tv_nsec + other.tv_nsec,
        };
        if new_ts.tv_nsec >= consts::time::NSEC_PER_SEC {
            new_ts.tv_sec += 1;
            new_ts.tv_nsec -= consts::time::NSEC_PER_SEC;
        }
        new_ts
    }
}
