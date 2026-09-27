use tideorm::relations::{RelationPath, RelationTree};

#[test]
fn test_relation_path_single_segment_has_no_nested_path() {
    let path = RelationPath::parse("posts");

    assert_eq!(path.segments, ["posts"]);
    assert_eq!(path.root(), "posts");
    assert!(path.nested().is_none());
}

#[test]
fn test_relation_path_nested_strips_one_hop_at_a_time() {
    let path = RelationPath::parse("posts.comments.author");
    assert_eq!(path.segments, ["posts", "comments", "author"]);
    assert_eq!(path.root(), "posts");

    let comments = path.nested().expect("two hops should remain");
    assert_eq!(comments.segments, ["comments", "author"]);
    assert_eq!(comments.root(), "comments");

    let author = comments.nested().expect("one hop should remain");
    assert_eq!(author.segments, ["author"]);
    assert!(author.nested().is_none());
}

#[test]
fn test_relation_path_drops_empty_segments() {
    let empty = RelationPath::parse("");
    assert!(empty.segments.is_empty());
    assert_eq!(empty.root(), "");

    assert_eq!(
        RelationPath::parse(".posts..comments.").segments,
        ["posts", "comments"]
    );
}

#[test]
fn test_relation_tree_ignores_empty_paths() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse(""));
    tree.add_path(&RelationPath::parse("posts..comments"));

    assert_eq!(tree.roots(), ["posts"]);
    let posts = tree.get_nested("posts").expect("posts is a root");
    assert_eq!(posts.roots(), ["comments"]);
}

#[test]
fn test_relation_tree_starts_empty() {
    let tree = RelationTree::new();

    assert!(tree.is_empty());
    assert!(tree.roots().is_empty());
}

#[test]
fn test_relation_tree_single_path_is_a_leaf() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts"));

    assert_eq!(tree.roots(), ["posts"]);
    assert!(
        tree.get_nested("posts")
            .expect("posts is a root")
            .is_empty()
    );
}

#[test]
fn test_relation_tree_merges_shared_prefixes() {
    let mut tree = RelationTree::new();
    tree.add_path(&RelationPath::parse("posts"));
    tree.add_path(&RelationPath::parse("profile"));
    tree.add_path(&RelationPath::parse("posts.comments"));
    tree.add_path(&RelationPath::parse("posts.comments.author"));

    let mut roots = tree.roots();
    roots.sort();
    assert_eq!(roots, ["posts", "profile"]);
    assert!(
        tree.get_nested("profile")
            .expect("profile is a root")
            .is_empty()
    );
    assert!(tree.get_nested("comments").is_none());

    let posts = tree.get_nested("posts").expect("posts is a root");
    assert_eq!(posts.roots(), ["comments"]);
    let comments = posts
        .get_nested("comments")
        .expect("comments is nested under posts");
    assert_eq!(comments.roots(), ["author"]);
}
