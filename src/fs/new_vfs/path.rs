//! From Oops: utils/path.rs
use crate::tools::errors::SysResult;
use alloc::format;
use alloc::{collections::VecDeque, string::String};
use core::fmt::{Debug, Formatter};
use core::ops::{Deref, DerefMut};

/// 代表一个已经正规化的路径, 即不存在 `.` 和 `..` 等成分的路径
/// 可以直接使用 `path[0]`, `path[1]`, ... 等来访问各个组成部分
/// 可以是绝对的, 也可以是相对的
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    is_absolute: bool,
    components: VecDeque<String>,
}

impl Debug for Path {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if self.is_absolute {
            write!(f, "/")?;
        }
        for (idx, p) in self.components.iter().enumerate() {
            if idx > 0 {
                write!(f, "/")?;
            }
            write!(f, "{}", p)?;
        }
        Ok(())
    }
}

impl From<&str> for Path {
    fn from(s: &str) -> Self {
        Self::from_str(s).unwrap()
    }
}

impl Deref for Path {
    type Target = VecDeque<String>;

    fn deref(&self) -> &Self::Target {
        &self.components
    }
}

impl DerefMut for Path {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.components
    }
}

impl Path {
    pub fn from_string(path: String) -> SysResult<Self> {
        // TODO: 找到到底哪里有可能使得路径解析失败了
        let is_absolute = path.starts_with('/');
        let mut components = VecDeque::new();
        for part in path.split('/') {
            match part {
                "" | "." => continue,
                ".." => {
                    if !components.is_empty() {
                        components.pop_back().unwrap();
                    }
                }
                _ => {
                    components.push_back(String::from(part));
                }
            }
        }

        Ok(Self {
            components,
            is_absolute,
        })
    }

    pub fn from_str(str: &str) -> SysResult<Self> {
        Path::from_string(String::from(str))
    }

    pub fn to_string(&self) -> String {
        format!("{:?}", self)
    }

    pub fn is_absolute(&self) -> bool {
        self.is_absolute
    }

    /// Whether it is the root
    pub fn is_root(&self) -> bool {
        self.is_absolute && self.components.is_empty()
    }

    /// Get the tail of the path
    pub fn last(&self) -> &String {
        if self.is_root() {
            panic!("is_root")
        }
        &self.components[self.len() - 1]
    }

    #[allow(unused)]
    pub fn first(&self) -> &String {
        &self.components[0]
    }

    /// Remove the head of the path
    #[allow(unused)]
    pub fn remove_head(&self) -> Self {
        if self.is_root() {
            panic!("already root")
        }
        let mut new = self.clone();
        new.pop_front();
        new
    }

    /// Remove the tail of the path
    pub fn remove_tail(&self) -> Self {
        if self.is_root() {
            panic!("already root")
        }
        let mut new = self.clone();
        new.pop_back();
        new
    }

    pub fn without_prefix(&self, prefix: &Path) -> Self {
        assert!(self.starts_with(prefix), "not prefix");
        let mut new = self.clone();
        // TODO: 检查是否是真的前缀
        for _ in 0..prefix.len() {
            new.pop_front();
        }
        new
    }

    /// Whether it is started with the prefix
    pub fn starts_with(&self, prefix: &Path) -> bool {
        if prefix.len() == 0 {
            return true;
        }
        if prefix.len() > self.len() {
            return false;
        }
        for (this_i, pre_i) in self.components.iter().zip(prefix.components.iter()) {
            if this_i != pre_i {
                return false;
            }
        }
        true
    }

    pub fn split_dir_file(&self) -> (Self, String) {
        let mut new = self.clone();
        let file = new.pop_back().unwrap();
        (new, file)
    }
}

#[allow(unused)]
pub fn path_test() {
    let path = Path::from_string(String::from("/a/b/c/d/")).unwrap();
    debug_assert_eq!(path.to_string(), "/a/b/c/d");
    let path = Path::from_string(String::from("/abcdefg/asdsd/asdasd")).unwrap();
    debug_assert_eq!(path.to_string(), "/abcdefg/asdsd/asdasd");
    let path = Path::from_string(String::from("aa/../bb/../cc/././."));
    debug_assert!(path.is_ok());
    debug_assert_eq!(path.unwrap().to_string(), "cc");

    debug_assert_eq!(
        Path::from_string(String::from("///")).unwrap().to_string(),
        "/"
    );
    debug_assert_eq!(
        Path::from_string(String::from("//a//.//b///c//")).unwrap().to_string(),
        "/a/b/c"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/a/../")).unwrap().to_string(),
        "/"
    );
    debug_assert_eq!(
        Path::from_string(String::from("a/../")).unwrap().to_string(),
        ""
    );
    debug_assert_eq!(
        Path::from_string(String::from("a/..//..")).unwrap().to_string(),
        ""
    );
    debug_assert_eq!(
        Path::from_string(String::from("././a")).unwrap().to_string(),
        "a"
    );
    debug_assert_eq!(
        Path::from_string(String::from(".././a")).unwrap().to_string(),
        "a"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/././a")).unwrap().to_string(),
        "/a"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/abc/../abc")).unwrap().to_string(),
        "/abc"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test")).unwrap().to_string(),
        "/test"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/")).unwrap().to_string(),
        "/test"
    );
    debug_assert_eq!(
        Path::from_string(String::from("test/")).unwrap().to_string(),
        "test"
    );
    debug_assert_eq!(
        Path::from_string(String::from("test")).unwrap().to_string(),
        "test"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//")).unwrap().to_string(),
        "/test"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/foo")).unwrap().to_string(),
        "/test/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/foo/")).unwrap().to_string(),
        "/test/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/foo/bar")).unwrap().to_string(),
        "/test/foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/foo/bar//")).unwrap().to_string(),
        "/test/foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//foo/bar//")).unwrap().to_string(),
        "/test/foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//./foo/bar//")).unwrap().to_string(),
        "/test/foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//./.foo/bar//")).unwrap().to_string(),
        "/test/.foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//./..foo/bar//")).unwrap().to_string(),
        "/test/..foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test//./../foo/bar//")).unwrap().to_string(),
        "/foo/bar"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/../foo")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/test/bar/../foo")).unwrap().to_string(),
        "/test/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("../foo")).unwrap().to_string(),
        "foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("../foo/")).unwrap().to_string(),
        "foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/../foo")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/../foo/")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/../../foo")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/bleh/../../foo")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/bleh/bar/../../foo")).unwrap().to_string(),
        "/foo"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/bleh/bar/../../foo/..")).unwrap().to_string(),
        "/"
    );
    debug_assert_eq!(
        Path::from_string(String::from("/bleh/bar/../../foo/../meh"))
            .unwrap()
            .to_string(),
        "/meh"
    );
}
