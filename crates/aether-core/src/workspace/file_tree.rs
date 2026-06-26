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
        let offset = self.data.len() as u32;
        let len = s.len() as u16;
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
            is_expanded: kind == FileKind::Directory,
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
                    // 使用 unsafe 绕过借用检查：我们知道 parent_idx != last
                    let parent_ptr = self.nodes.as_mut_ptr();
                    unsafe {
                        (*parent_ptr.add(parent_idx_usize)).last_child = idx;
                        (*parent_ptr.add(last as usize)).next_sibling = idx;
                    }
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
