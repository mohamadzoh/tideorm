// A model maps one struct to one table: one error, not a page of E0107s
// from the generated impls.
#[tideorm::model(table = "boxes")]
struct Boxed<T> {
    #[tideorm(primary_key, auto_increment)]
    id: i64,
    #[tideorm(skip)]
    marker: std::marker::PhantomData<T>,
}

fn main() {}
