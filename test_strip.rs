fn main() {
    let t1 = "Option<i64>";
    let t2 = "Option < i64 >";
    
    let inner1 = t1.strip_prefix("Option<").and_then(|s| s.strip_suffix(">")).unwrap_or(t1);
    let inner2 = t2.strip_prefix("Option<").and_then(|s| s.strip_suffix(">")).unwrap_or(t2);
    
    println!("t1 inner: '{}'", inner1);
    println!("t2 inner: '{}'", inner2);
}
