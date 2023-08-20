# `/sys/kernel/debug/kcov` 实现

DebugFS 是一个类似 procfs 的文件系统，用于内核调试。它的文件系统类型是 debugfs，挂载点是 `/sys/kernel/debug`。

在 Debugfs 中, 最重要的一个文件是 `kcov`, 它基于插桩技术, 能自动记录内核函数执行的顺序并写入该文件, 极大地便利了内核调试.

在 MankorOS 中, 我们基于手动插桩技术, 实现了该功能. 在执行了一些操作后, `busybox cat /sys/kernel/debug/kcov` 即可看到内核中关键函数的 sp.