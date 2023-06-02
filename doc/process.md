# 进程设计

MankorOS 支持进程和线程, 进程和线程都是用轻量级线程 (`LightProcess`) 结构统一表示. 
<!-- TODO: 进程管理模块没有, CPU 模块没有 -->

在本章中, 将先后介绍:

- `LightProcess` 结构体
<!-- TODO: 进程调度算法 -->
<!-- TODO: 进程同步机制 (信号) -->
- 中断和异常处理

## 进程和线程

与 Linux 内核相似, 在 MankorOS 内核中，进程和线程两者并没有区别，可以统一地称之为轻量级进程, 以 `LightProcess` 结构体表示和管理, 线程可以理解为共享了资源的进程.
`sys_clone` 系统调用时，能够指定子进程和父进程之间共享的资源 (包括地址空间、文件描述符表、待处理信号等)。
线程模型的具体定义可以由用户库负责。

### `LightProcess` 结构体

目前 `LightProcess` 结构体主要包含以下数据结构 (其中 **粗体** 的是可以在任务之间共享的)

- 进程基本信息
  - 进程号 `id`
  - 进程状态 `state`
  - 退出码 `exit_code`
- 进程关系信息
  - 父进程 `parent`
  - **子进程数组 `children`**
  - **进程组信息 `group`** (用于实现线程组)
- 进程资源信息
  - **地址空间 `memory`**
  - **文件系统信息 `fsinfo`**
  - **文件描述符表 `fdtable`**
- 其他
  - **信号处理函数 `signal`**
  <!-- TODO: 时间相关 -->

`LightProcess` 代码如下所示:
<!-- TODO: 修改 -->

```rust
type Shared<T> = Arc<SpinNoIrqLock<T>>;

pub struct LightProcess {
    id: PidHandler,
    parent: Shared<Option<Weak<LightProcess>>>,
    context: SyncUnsafeCell<Box<UKContext, Global>>,

    children: Arc<SpinNoIrqLock<Vec<Arc<LightProcess>>>>,
    status: SpinNoIrqLock<SyncUnsafeCell<ProcessStatus>>,
    exit_code: AtomicI32,

    group: Shared<ThreadGroup>,
    memory: Shared<UserSpace>,
    fsinfo: Shared<FsInfo>,
    fdtable: Shared<FdTable>,
    signal: SpinNoIrqLock<signal::SignalSet>,
}
```

在内核代码中，其他部分一般持有 `Arc<LightProcess>` (`LightProcess` 的引用计数智能指针).
这样既可以保证对应进程的信息不会过早被释放, 也可以保证当无人持有此进程信息时, 此结构体占用的资源可以被回收. 
`LightProcess` 中可以共享的数据结构都用 `Arc` 包装，
在 `sys_clone` 系统调用的实现中, 如果需要共享特定资源, 
则可以直接利用 `Arc::clone` 方法使得两个进程的数据结构指向同一个实例;
如果无需共享, 则使用具体资源的 `clone` 的方法进行复制:

```rust
// src/process/lproc.rs:265 
if flags.contains(CloneFlags::THREAD) {
    parent = self.parent.clone();
    children = self.children.clone();
    // remember to add the new lproc to group please!
    group = self.group.clone();
} else {
    parent = new_shared(Some(Arc::downgrade(&self)));
    children = new_shared(Vec::new());
    group = new_shared(ThreadGroup::new_empty());
}
```

### 进程的状态

在 MankorOS 中, 进程有 3 种状态:

- `UNINIT`: 该进程还未针对第一次运行做好准备 (没有为 `main` 函数准备好栈上的内容)
- `READY`: 该进程可以被执行
- `ZOMBIE`: 已经退出的进程

<!-- TODO: 状态转移图 -->

在异步内核中, 不需要维护进程是否被阻塞之类或是否已经被加入准备执行的队列的状态:

- 若进程执行某个耗时久的系统调用 (比如 `sleep`), 
  代表它的 Future 会直接返回 Pending, 从而使它离开调度队列,
  直到阻塞的条件被满足后, 会被 waker 自动重新加入回调度队列,
  因此不需要代表 "进程因为缺少某条件不能被调度" 的状态.
- 异步编程模型中的 `Task` 抽象 () 会保证一个 Future 不会被 wake 多次,
  从而使得已经在调度队列中的进程不会被重复加入.
  因此不需要代表 "进程已经被调度" 的状态 


## 地址空间

出于性能考虑, MankorOS 的内核与用户程序共用页表, 
且内核空间占用的二级页表在不同用户程序之间是共享的.

<!-- TODO: 插一个共享页表的图示水水页数 -->

### 地址空间布局

<!-- TODO: 加入图片 -->

### 地址空间管理

MankorOS 中的地址空间的各类信息由 `UserSpace` 结构体表示:

```rust
pub struct UserSpace {
    // 根页表
    pub page_table: PageTable,
    // 分段管理
    areas: UserAreaManager,
}
```

其中 `UserAreaManager` 结构体用于管理用户程序的各个段, 其组成非常简单:

```rust
pub struct UserAreaManager {
    map: RangeMap<VirtAddr, UserArea>,    
}

pub struct RangeMap<U: Ord + Copy, V>(BTreeMap<U, Node<U, V>>);
```

`RangeMap` 的实现直接借用了 [FTL-OS](https://gitee.com/ftl-os/ftl-os/blob/master/code/kernel/src/tools/container/range_map.rs) 的实现.
但额外增加了原地修改区间长度的 `extend_back` 和 `reduce_back` 方法,
以针对堆内存的动态增长和缩减进行优化.
对于其他类型的区间长度增减, 
仍然采用 "创建新区间-合并" 和 "分裂旧区间-删除" 的方式.

`UserArea` 中保存了各个内存段的信息, 包括:

<!-- TODO: 看看这个 MMAP_SHARED 的段的 todo 要不要放上来 -->

```rust
bitflags! {
    pub struct UserAreaPerm: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

enum UserAreaType {
    /// 匿名映射区域
    MmapAnonymous,
    /// 私有映射区域
    MmapPrivate {
        file: Arc<dyn VfsNode>,
        offset: usize,
    },
    // TODO: 共享映射区域
    // MmapShared {
    //     file: Arc<dyn VfsNode>,
    //     offset: usize,
    // },
}

pub struct UserArea {
    kind: UserAreaType,
    perm: UserAreaPerm,
}
```

考虑到地址空间段的类型是基本确定的, 
此处并没有像 Linux 一样使用函数指针 ("虚表") 来抽象各类段的行为, 
也没有使用本质上相同的 Rust 的 `dyn trait` 方式,
而是直接使用枚举类型实现.
这样既可以保证处理时的完整地好各类情况, 也有一定的性能优势.

MankorOS 中所有段都是懒映射且懒加载的,
所有内存数据都会且只会在处理缺页异常时被请求 
(譬如换入页或读取文件信息).
这同时也带来了 `exec` 系统调用中对 ELF 文件的懒加载能力.

该实现还意味着各种不同类型的段只需要在构造或处理缺页异常时进行不同的处理即可.
几乎所有 `UserArea` 方法中只有缺页异常处理需要用到 `match (self.kind)`,
使得使用枚举区分不同段的方法带来了几乎为零的代码清晰程度开销.
正因如此, 我们放弃了虚表方法可能带来的代码清晰度的提升与灵活性, 
而选择了性能更好的枚举实现.