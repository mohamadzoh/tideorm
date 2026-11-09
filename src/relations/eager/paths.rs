//! The relation paths an eager load is asked for, merged into one tree.

use std::collections::HashMap;

/// A dotted relation path such as `"posts.comments.author"`, split into the
/// hops an eager load walks.
///
/// Purely syntactic: nothing here checks that the names exist. An unknown
/// segment surfaces later, from the generated eager loader, as
/// "Unknown relation '..' on ..".
#[derive(Debug, Clone)]
pub struct RelationPath {
    /// The dot-separated hops, outermost first.
    pub segments: Vec<String>,
}

impl RelationPath {
    /// Split a dotted path into segments, dropping empty ones: `""` parses to
    /// no segments and `"posts..comments"` to two.
    pub fn parse(path: &str) -> Self {
        Self {
            segments: path
                .split('.')
                .filter(|segment| !segment.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// The first hop — the relation to resolve on the model at hand. Empty
    /// string for a path with no segments.
    pub fn root(&self) -> &str {
        self.segments.first().map_or("", String::as_str)
    }

    /// The path with [`root`](Self::root) stripped, to continue with on the
    /// related model. `None` once a single hop is left.
    pub fn nested(&self) -> Option<RelationPath> {
        if self.segments.len() > 1 {
            Some(RelationPath {
                segments: self.segments[1..].to_vec(),
            })
        } else {
            None
        }
    }
}

/// The merged set of relation paths one eager load will resolve.
///
/// Several [`RelationPath`]s are folded into a trie, so `.with("posts.comments")`
/// and `.with("posts.author")` share a single load of `posts` and then branch.
/// The loader walks it level by level, batching all parents at each level, which
/// is why breadth is free and only depth costs extra round trips.
#[derive(Debug, Clone, Default)]
pub struct RelationTree {
    children: HashMap<String, RelationTree>,
}

impl RelationTree {
    /// An empty tree — an eager load against it resolves nothing and skips the
    /// loader entirely.
    #[must_use]
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
        }
    }

    /// Merge a path into the tree, sharing every prefix it has in common with
    /// paths already added. Re-adding the same path is a no-op; an empty path is
    /// ignored.
    pub fn add_path(&mut self, path: &RelationPath) {
        if path.segments.is_empty() {
            return;
        }

        let root = path.root().to_string();
        let child = self.children.entry(root).or_default();

        if let Some(nested) = path.nested() {
            child.add_path(&nested);
        }
    }

    /// The relations to resolve at this level.
    ///
    /// Backed by a `HashMap`, so the order is unspecified and varies between
    /// runs; relations at one level are independent, but do not depend on the
    /// order for anything observable.
    pub fn roots(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    /// The subtree to resolve on `name`'s related models, or `None` if `name` is
    /// not in this level at all.
    ///
    /// A leaf yields `Some` of an empty tree, which the loader treats as "stop
    /// here".
    pub fn get_nested(&self, name: &str) -> Option<&RelationTree> {
        self.children.get(name)
    }

    /// Whether this level has nothing to resolve.
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }
}
