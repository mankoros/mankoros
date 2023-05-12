use bitflags::bitflags;

bitflags! {
    pub struct SignalSet: u32 {
        const SIGHUP    = 1 << (     0);   // 用户终端连接结束
        const SIGINT    = 1 << ( 2 - 1);   // 程序终止 可能是Ctrl+C
        const SIGQUIT   = 1 << ( 3 - 1);   // 类似SIGINT Ctrl+\
        const SIGILL    = 1 << ( 4 - 1);   // 执行了非法指令 页错误 栈溢出
        const SIGTRAP   = 1 << ( 5 - 1);   // 断点指令产生 debugger使用
        const SIGABRT   = 1 << ( 6 - 1);   // abort函数产生
        const SIGBUS    = 1 << ( 7 - 1);   // 非法地址或地址未对齐
        const SIGFPE    = 1 << ( 8 - 1);   // 致命算数运算错误，浮点或溢出或除以0
        const SIGKILL   = 1 << ( 9 - 1);   // 强制立刻结束程序执行
        const SIGUSR1   = 1 << (10 - 1);   // 用户保留1
        const SIGSEGV   = 1 << (11 - 1);   // 试图读写未分配或无权限的地址
        const SIGUSR2   = 1 << (12 - 1);   // 用户保留2
        const SIGPIPE   = 1 << (13 - 1);   // 管道破裂，没有读管道
        const SIGALRM   = 1 << (14 - 1);   // 时钟定时信号
        const SIGTERM   = 1 << (15 - 1);   // 程序结束信号，用来要求程序自己正常退出
        const SIGSTKFLT = 1 << (16 - 1);   //
        const SIGCHLD   = 1 << (17 - 1);   // 子进程结束时父进程收到这个信号
        const SIGCONT   = 1 << (18 - 1);   // 让停止的进程继续执行，不能阻塞 例如重新显示提示符
        const SIGSTOP   = 1 << (19 - 1);   // 暂停进程 不能阻塞或忽略
        const SIGTSTP   = 1 << (20 - 1);   // 暂停进程 可处理或忽略 Ctrl+Z
        const SIGTTIN   = 1 << (21 - 1);   // 当后台作业要从用户终端读数据时, 该作业中的所有进程会收到SIGTTIN信号. 缺省时这些进程会停止执行
        const SIGTTOU   = 1 << (22 - 1);   // 类似于SIGTTIN, 但在写终端(或修改终端模式)时收到.
        const SIGURG    = 1 << (23 - 1);   // 有"紧急"数据或out-of-band数据到达socket时产生.
        const SIGXCPU   = 1 << (24 - 1);   // 超过CPU时间资源限制 可以由getrlimit/setrlimit来读取/改变。
        const SIGXFSZ   = 1 << (25 - 1);   // 进程企图扩大文件以至于超过文件大小资源限制
        const SIGVTALRM = 1 << (26 - 1);   // 虚拟时钟信号, 类似于SIGALRM, 但是计算的是该进程占用的CPU时间.
        const SIGPROF   = 1 << (27 - 1);   // 类似于SIGALRM/SIGVTALRM, 但包括该进程用的CPU时间以及系统调用的时间
        const SIGWINCH  = 1 << (28 - 1);   // 窗口大小改变时发出
        const SIGIO     = 1 << (29 - 1);   // 文件描述符准备就绪, 可以开始进行输入/输出操作.
        const SIGPWR    = 1 << (30 - 1);   // Power failure
        const SIGSYS    = 1 << (31 - 1);   // 非法的系统调用
        const SIGTIMER  = 1 << (32 - 1);   // 非法的系统调用
    }
}
