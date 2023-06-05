# 基于无栈协程的异步

## 基础概念

### `Future`

无栈协程的核心是 `Future`:

```rust
pub trait Future {
    type Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}
```

`Future` 是异步函数的抽象. 
调用 `poll` 方法代表 "检查任务结果", 
如果任务已经完成, 则返回 `Poll::Ready` (其中包含结果数据), 
否则返回 `Poll::Pending` 表示任务尚未结束.

```rust
struct IdFuture {
    result: i32
}
impl Future for IdFuture {
    type Output = i32;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if /* some condition */ {
            Poll::Ready(self.result)
        } else {
            Poll::Pending
        }
    }
}
impl IdFuture {
    pub fn new(result: i32) -> Self {
        Self { result }
    }
}
```

`Future` 之间可以组合.
下面的 `Future` 实现了将一个 `Future` 的结果乘以 2 的功能:

```rust
struct DoubleFuture {
    a: IdFuture,
}
impl Future for DoubleFuture {
    type Output = i32;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };

        // 调用 a 的 poll 方法, 如果 a 返回 Poll::Pending, 则返回 Poll::Pending
        let a = unsafe { Pin::new_unchecked(&mut this.a) };
        let ar = match a.poll(cx) {
            Poll::Ready(x) => x,
            Poll::Pending => return Poll::Pending,
        };

        // 如果 a 返回了 Poll::Ready, 就返回最终结果 Poll::Ready(ar * 2)
        Poll::Ready(ar * 2)
    }
}
impl DoubleFuture {
    pub fn new(x: i32) -> Self {
        Self { a: IdFuture::new(x) }
    }
}
```

可以看到, `Future` 的组合是通过对子 `Future` 的 `poll` 方法结果进行简单的组合得到的:
只要子 `Future` 返回 `Poll::Pending`, 则父 `Future` 也返回 `Poll::Pending`.
于是我们可以让编译器代我们生成上述代码:

```rust
async fn double(x: i32) -> i32 {
    let ar = IdFuture::new(x).await;
    ar * 2
}
```

一个异步函数的上下文可以被保存在具体的 `Future` 结构体中, 
从而使得函数可保存其上下文状态并恢复之.
比如下面的 `Future` 实现了将两个 `Future` 的结果相加的功能:

```rust
struct AddFuture {
    status: usize,
    x: i32,
    y: i32,
    a1: IdFuture,
    a2: IdFuture,
}
impl Future for AddFuture {
    type Output = i32;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        loop {
            // 使用状态机的方式实现
            match this.status {
                AddFuture::STATUS_BEGIN => {
                    let a1 = unsafe { Pin::new_unchecked(&mut this.a1) };
                    let ar = match a1.poll(cx) {
                        Poll::Ready(x) => x,
                        Poll::Pending => return Poll::Pending,
                    };
                    // 保存局部变量
                    this.x = ar;
                    // 修改状态
                    this.status = AddFuture::STATUS_A1;
                }
                AddFuture::STATUS_A1 => {
                    let a2 = unsafe { Pin::new_unchecked(&mut this.a2) };
                    let ar = match a2.poll(cx) {
                        Poll::Ready(x) => x,
                        Poll::Pending => return Poll::Pending,
                    };
                    // 保存局部变量
                    this.y = ar;
                    // 修改状态
                    this.status = AddFuture::STATUS_A2;
                }
                AddFuture::STATUS_A2 => {
                    // 返回最终结果
                    return Poll::Ready(this.x + this.y);
                }
                _ => unreachable!()
            }
        }
    }
}
const UNINIT: i32 = 0;
impl AddFuture {
    const STATUS_BEGIN: usize = 0;
    const STATUS_A1: usize = 1;
    const STATUS_A2: usize = 2;

    pub fn new(x: i32, y: i32) -> Self {
        Self { 
            status: AddFuture::STATUS_BEGIN, 
            x: UNINIT,
            y: UNINIT,
            a1: IdFuture::new(x), 
            a2: IdFuture::new(y) 
        }
    }
}
```

注意我们不能直接顺序地调用两个 `Future` 的 `poll` 方法并检查,
因为一个 `Future` 的 `poll` 方法可能会被执行多次.
比如第一次 `poll` 时, `a1` 返回了 `Ready` 但 `a2` 返回了 `Pending`,
此时整个 `poll` 也应该返回 `Pending`.
但是当第二次调用 `poll` 时,
我们显然不能再重复去调用 `a1` 的 `poll` 方法,
所以我们需要保存 "我们已经执行过 `a1.poll` 了" 这个状态,
同时还要保存 `a1.poll` 方法的返回值.

于是我们可以总结出, 
每次对子 `Future` 的 `poll` 方法调用, 都需要产生一个 "保存点",
并且还需要在这里存下 `poll` 方法的返回值.
这种规则也是非常机械的,
我们还是可以交给编译器, 让它在编译时为我们自动生成相似的代码:

```rust
async fn add(x: i32, y: i32) -> i32 {
    let a1 = IdFuture::new(x).await;
    let a2 = IdFuture::new(y).await;
    a1 + a2
}
```

### `Context` & `Waker`

<!-- TODO -->

### `Task`

<!-- TODO -->

### `Executor`

<!-- TODO -->

## 内核实现要点

倘若是编写普通的异步程序,
只需使用使用 `async`/`await` 关键字即可.
但一个异步内核显然不能仅仅依赖组合已有的 `Future`,
还必须实现一些底层或顶层的 `Future`,
这些 `Future` 大致可以分为三类:

- 为其他 `Future` "装饰" 的 `Future`
- 为底层回调提供包装的 `Future`
- 一些辅助性的工具 `Future`

### 装饰型 `Future`

当我们使用 `async`/`await` 时,
编译器会自动为我们生成一个 `Future` 的实现,
这个实现会在子 `Future` 返回 `Pending` 时直接返回 `Pending`.

如果我们需要在子 `Future` 返回 `Pending` 时执行一些额外的操作,
我们就必须手动编写该 `Future` 的实现.
异步内核中用于切换到用户态线程的 `Future` 就是典型的此类 `Future`,
无论子 `Future` 返回 `Pending` 还是 `Ready`,
它都需要在执行前后完成一些额外的操作:

```rust
pub struct OutermostFuture<F: Future> {
    lproc: Arc<LightProcess>,
    future: F,
}
impl<F: Future> Future for OutermostFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // ... (关闭中断, 切换页表, 维护当前 hart 状态)
        let ret = unsafe { Pin::new_unchecked(&mut this.future).poll(cx) };
        // ... (开启中断, 恢复页表, 恢复之前的 hart 状态)
        ret
    }
}
```

随后我们便可以在 `userloop` 外边包裹这个 `Future`,
从而其无需再担心进程切换相关的杂事:

```rust
pub fn spawn_proc(lproc: Arc<LightProcess>) {
    // userloop 为切换到用户态执行的 Future
    let future = OutermostFuture::new(
        lproc.clone(), userloop::userloop(lproc));
    let (r, t) = executor::spawn(future);
    r.schedule();
    t.detach();
}
```

### 包装型 `Future`

这种 `Future` 通常位于 `Future` 栈的最底层 (最后被调用的那个),
用于将底层的回调接口包装成 `Future`.
其一般表现为将 `Waker` 传出或将 `|| cx.waker().wake_by_ref()` 设置为回调函数.
异步内核中用于实现异步管道读写操作的 `Future` 就是典型的此类 `Future`:

```rust
pub struct PipeReadFuture {
    pipe: Arc<Pipe>,
    buf: Arc<[u8]>,
    offset: usize,
}
impl Future for PipeReadFuture {
    type Output = usize;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // ... 各种检查和杂项代码
        if pipe.is_empty() {
            // 如果管道为空, 就将当前 waker 存起来. 在管道写入数据之后, 
            // 会调用 pipe.read_waker.wake_by_ref() 以重新将顶层 Future 唤醒
            pipe.read_waker = Some(cx.waker().clone());
            Poll::Pending
        } else if pipe.is_done() {
            pipe.read_waker = None;
            Poll::Ready(0)
        } else {
            let len = pipe.read(this.buf.as_mut(), this.offset);
            // 如果写入时写满了管道的缓冲区, 那么就将写入者的 waker 存起来.
            // 现在再调用. 如果写入者已经写完了, 则它不会再设置 pipe 的该成员.
            if let Some(write_waker) = pipe.write_waker {
                // 如果管道写入数据之前, 已经有一个 waker 等待管道读取数据,
                // 那么就将这个 waker 唤醒
                write_waker.wake_by_ref();
            }
            Poll::Ready(len)
        }
    }
}
```

### 辅助型 `Future`

除了上面两大类 `Future` 之外,
还有一些工具性质的 `Future`,
在开发异步内核时也是非常有用的, 现列举一二.

#### `YieldFuture`

有时候, 我们需要当前 `Future` 主动返回一次 `Pending` 以让出控制权,
但是并不想让它等待什么, 而是直接回到调度器中等待下一次调度.
这时候就可以使用 `YieldFuture`:

```rust
pub struct YieldFuture(bool);
impl Future for YieldFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0 {
            // 之后再次被 poll 时, 它直接返回 Ready, 什么事都不干
            return Poll::Ready(());
        } else {
            // 第一次调用时, self.0 为 false, 此时它直接调用 wake_by_ref
            // 将自己重新加回调度器中, 并返回 Pending 使得所有上层 Future
            // 返回, 让出这一轮的调度权
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
pub fn yield_now() -> YieldFuture {
    YieldFuture(false)
}
```

它可以用于实现 `yield` 系统调用, 
也可以在异步内核实现过程中用来实现某种 "spin" 式操作:

```rust
loop {
    let resource_opt = try_get_resouce();
    if let Some(resource) = resource_opt {
        break resource;
    } else {
        // 如果资源不可用, 就让出控制权, 并期望下次被调用时等待资源可用
        yield_now().await;
    }
}
```

但是, 这种写法是不推荐的, 
它放弃了异步内核的很大一部分优越性.
在使用这种写法之前, 应该首先尝试将该资源的获取改写为回调式的,
使用 "包装型 `Future`" 的写法实现.

`YieldFuture` 也可用于某些系统的最底层实现中,
比如搭配定时器中断, 使用自旋检查的方法实现内核内的定时任务.

### `WakerFuture`

`WakerFuture` 用于在 `async fn` 中获取当前 `Waker`:

```rust
struct WakerFuture;
impl Future for WakerFuture {
    type Output = Waker;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(cx.waker().clone())
    }
}
```

如下使用便可以在 `async fn` 中获取当前 `Waker`:

```rust
async fn foo() {
    let waker = WakerFuture.await;
    resource.setReadyCallback(|| waker.wake_by_ref());
}
```

使用该 `Future` 时, 
可以使很大一部分包装型 `Future` 得以直接使用 `async fn` 来实现,
而不用再手动实现 `Future` trait.

## 上下文切换

<!-- TODO: trap 相关和 syscall (通用) 的实现 -->