// copy from linux sys/time.h
// struct timeval {
//     time_t      tv_sec;     /* seconds */
//     suseconds_t tv_usec;    /* microseconds */
// };

use core::ops::Add;

use crate::consts;

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default, Debug)]
pub struct TimeVal {
    // seconds
    pub tv_sec: usize,
    // microseconds
    pub tv_usec: usize,
}

impl TimeVal {
    pub fn now() -> Self {
        super::get_time_us().into()
    }

    pub fn time_in_ms(&self) -> usize {
        self.tv_sec * 1000 + self.tv_usec / 1000
    }
}

impl Add for TimeVal {
    type Output = TimeVal;
    fn add(self, other: Self) -> Self {
        let mut new_ts = Self {
            tv_sec: self.tv_sec + other.tv_sec,
            tv_usec: self.tv_usec + other.tv_usec,
        };
        if new_ts.tv_usec >= consts::time::USEC_PER_SEC {
            new_ts.tv_sec += 1;
            new_ts.tv_usec -= consts::time::USEC_PER_SEC;
        }
        new_ts
    }
}

impl From<usize> for TimeVal {
    fn from(usec: usize) -> Self {
        Self {
            tv_sec: usec / consts::time::USEC_PER_SEC,
            tv_usec: usec % consts::time::USEC_PER_SEC,
        }
    }
}

impl From<TimeVal> for usize {
    fn from(val: TimeVal) -> Self {
        val.tv_sec * consts::time::USEC_PER_SEC + val.tv_usec
    }
}
