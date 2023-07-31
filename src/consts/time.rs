use super::const_register::{register_const, register_fn, register_mut_const};

register_mut_const!(
    /// 时钟频率，和平台有关
    /// 目前硬编码为 10MHz(for qemu)
    CLOCK_FREQ,
    usize,
    10_000_000
);
register_const!(
    /// 每秒的时钟中断数
    INTERRUPT_PER_SEC,
    usize,
    100
);
register_const!(
    /// 每秒有多少微秒
    USEC_PER_SEC,
    usize,
    1_000_000
);
register_fn!(
    /// 每微秒的时钟周期数
    machine_ticks_per_usec,
    usize,
    CLOCK_FREQ / USEC_PER_SEC
);
register_const!(
    /// 每个时钟中断占多少微秒
    USEC_PER_INTERRUPT,
    usize,
    USEC_PER_SEC / INTERRUPT_PER_SEC
);
register_const!(
    /// 每秒的纳秒数
    NSEC_PER_SEC,
    usize,
    1_000_000_000
);
register_fn!(
    /// 每个时钟周期需要多少纳秒 (取整)
    nsec_per_machine_ticks,
    usize,
    NSEC_PER_SEC / CLOCK_FREQ
);
register_const!(
    /// 当 nsec 为这个特殊值时，指示修改时间为现在
    UTIME_NOW,
    usize,
    0x3fffffff
);
register_const!(
    /// 当 nsec 为这个特殊值时，指示不修改时间
    UTIME_OMIT,
    usize,
    0x3ffffffe
);
