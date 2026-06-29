/// 文件树节点（紧凑内存布局）
pub struct FileTree {
    /// 扁平存储所有节点（Vec而非树指针，缓存友好）
    nodes: Vec<FileNode>,
    /// 字符串池：所有路径名共享存储
    names: StringPool,
    /// 虚拟根节点的 first_child（所有 parent_idx = u32::MAX 的节点通过 sibling 链连接）
    root_first_child: u32,
    /// 虚拟根节点的 last_child（用于 O(1) 尾插入）
    root_last_child: u32,
}

/// 单个节点（紧凑内存布局）
#[derive(Clone, Debug)]
pub struct FileNode {
    pub name_offset: u32,
    pub name_len: u16,
    pub kind: FileKind,
    pub parent_idx: u32,
    pub first_child: u32,
    pub last_child: u32,
    pub next_sibling: u32,
    pub depth: u8,
    pub is_expanded: bool,
    /// 目录的子节点是否已扫描加载（懒加载标记）
    /// false 表示该目录尚未扫描子节点，展开时需先加载
    pub is_loaded: bool,
    pub is_git_tracked: bool,
    pub is_git_modified: bool,
    pub file_size: u64,
    pub modified_time: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

/// 字符串池
#[derive(Clone, Debug)]
pub struct StringPool {
    data: String,
}

impl StringPool {
    pub fn new() -> Self {
        Self {
            data: String::new(),
        }
    }

    pub fn add(&mut self, s: &str) -> (u32, u16) {
        // M-15: 用 try_from 检查溢出，避免静默截断导致后续 get() 返回错误数据
        let offset = u32::try_from(self.data.len())
            .expect("M-15: StringPool offset overflow (pool data > 4GB)");
        let len =
            u16::try_from(s.len()).expect("M-15: StringPool length overflow (single name > 64KB)");
        self.data.push_str(s);
        (offset, len)
    }

    pub fn get(&self, offset: u32, len: u16) -> &str {
        &self.data[offset as usize..(offset + len as u32) as usize]
    }
}

impl FileTree {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            names: StringPool::new(),
            root_first_child: u32::MAX,
            root_last_child: u32::MAX,
        }
    }

    pub fn add_node(&mut self, name: &str, kind: FileKind, parent_idx: u32, depth: u8) -> u32 {
        let (name_offset, name_len) = self.names.add(name);
        let idx = self.nodes.len() as u32;
        self.nodes.push(FileNode {
            name_offset,
            name_len,
            kind,
            parent_idx,
            first_child: u32::MAX,
            last_child: u32::MAX,
            next_sibling: u32::MAX,
            depth,
            // 只有第一层目录（depth==0）默认展开，减少打开文件夹时的初始节点数
            is_expanded: kind == FileKind::Directory && depth == 0,
            // 新建节点默认未加载子节点；open_folder 会对根层显式标记
            is_loaded: false,
            is_git_tracked: false,
            is_git_modified: false,
            file_size: 0,
            modified_time: 0,
        });

        // 更新父节点的first_child链表 - O(1) 尾指针插入
        if parent_idx != u32::MAX {
            let parent_idx_usize = parent_idx as usize;
            if parent_idx_usize < self.nodes.len() {
                // 先读取 last_child 值，避免同时借用
                let last_child_opt = {
                    let parent = &self.nodes[parent_idx_usize];
                    if parent.first_child == u32::MAX {
                        None
                    } else {
                        Some(parent.last_child)
                    }
                };

                if let Some(last) = last_child_opt {
                    let last_usize = last as usize;
                    // C-02: 用 split_at_mut 消除 unsafe 指针别名，保证两个可变引用指向不同元素
                    debug_assert_ne!(
                        parent_idx_usize, last_usize,
                        "C-02: parent_idx 与 last_child 相同会导致别名 UB（数据损坏）"
                    );
                    if parent_idx_usize < last_usize {
                        let (left, right) = self.nodes.split_at_mut(last_usize);
                        left[parent_idx_usize].last_child = idx;
                        right[0].next_sibling = idx;
                    } else if parent_idx_usize > last_usize {
                        let (left, right) = self.nodes.split_at_mut(parent_idx_usize);
                        left[last_usize].next_sibling = idx;
                        right[0].last_child = idx;
                    }
                    // parent_idx_usize == last_usize 时数据损坏，不修改避免 UB
                } else {
                    let parent = &mut self.nodes[parent_idx_usize];
                    parent.first_child = idx;
                    parent.last_child = idx;
                }
            }
        } else {
            // parent_idx == u32::MAX: 挂到虚拟根节点下
            if self.root_first_child == u32::MAX {
                self.root_first_child = idx;
                self.root_last_child = idx;
            } else {
                let last = self.root_last_child;
                self.nodes[last as usize].next_sibling = idx;
                self.root_last_child = idx;
            }
        }

        idx
    }

    pub fn get_node(&self, idx: u32) -> Option<&FileNode> {
        self.nodes.get(idx as usize)
    }

    pub fn get_node_mut(&mut self, idx: u32) -> Option<&mut FileNode> {
        self.nodes.get_mut(idx as usize)
    }

    pub fn get_name(&self, node: &FileNode) -> &str {
        self.names.get(node.name_offset, node.name_len)
    }

    pub fn iter_children(&self, parent_idx: u32) -> FileTreeIterator<'_> {
        let first = if parent_idx == u32::MAX {
            // 虚拟根节点：使用 root_first_child
            if self.root_first_child != u32::MAX {
                Some(self.root_first_child)
            } else {
                None
            }
        } else {
            self.nodes
                .get(parent_idx as usize)
                .map(|n| n.first_child)
                .filter(|&c| c != u32::MAX)
        };

        FileTreeIterator {
            tree: self,
            current: first,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 遍历所有节点（用于懒加载预扫描等）
    pub fn nodes_iter(&self) -> impl Iterator<Item = &FileNode> {
        self.nodes.iter()
    }

    pub fn first_root_node(&self) -> Option<u32> {
        if self.root_first_child != u32::MAX {
            Some(self.root_first_child)
        } else {
            None
        }
    }
}

pub struct FileTreeIterator<'a> {
    tree: &'a FileTree,
    current: Option<u32>,
}

impl<'a> Iterator for FileTreeIterator<'a> {
    type Item = &'a FileNode;

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.current?;
        let node = self.tree.nodes.get(idx as usize)?;
        self.current = if node.next_sibling != u32::MAX {
            Some(node.next_sibling)
        } else {
            None
        };
        Some(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_tree() {
        let mut tree = FileTree::new();
        let root = tree.add_node("src", FileKind::Directory, u32::MAX, 0);
        let _main = tree.add_node("main.c", FileKind::File, root, 1);
        let _lib = tree.add_node("lib.c", FileKind::File, root, 1);

        assert_eq!(tree.len(), 3);

        let children: Vec<_> = tree.iter_children(root).collect();
        assert_eq!(children.len(), 2);
    }
}
