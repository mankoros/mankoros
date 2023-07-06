// copy from linux sys/time.h
// struct timeval {
//     time_t      tv_sec;     /* seconds */
//     suseconds_t tv_usec;    /* microseconds */
// };

use core::ops::Add;

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Default)]
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
}

impl Add for TimeVal {
    type Output = TimeVal;
    fn add(self, other: Self) -> Self {
        let mut new_ts = Self {
            tv_sec: self.tv_sec + other.tv_sec,
            tv_usec: self.tv_usec + other.tv_usec,
        };
        if new_ts.tv_usec >= super::USEC_PER_SEC {
            new_ts.tv_sec += 1;
            new_ts.tv_usec -= super::USEC_PER_SEC;
        }
        new_ts
    }
}

impl From<usize> for TimeVal {
    fn from(usec: usize) -> Self {
        Self {
            tv_sec: usec / super::USEC_PER_SEC,
            tv_usec: usec % super::USEC_PER_SEC,
        }
    }
}

impl From<TimeVal> for usize {
    fn from(val: TimeVal) -> Self {
        val.tv_sec * super::USEC_PER_SEC + val.tv_usec
    }
}
