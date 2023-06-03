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

<!-- TODO: 介绍包 Future 的 Future, 包 Waker 的 Future, 包进程的 Future 三种主要 Future -->

## 上下文切换

<!-- TODO: trap 相关和 syscall (通用) 的实现 -->