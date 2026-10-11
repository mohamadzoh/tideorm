# Runnable examples

Each file is a complete program. SQLite examples use an in-memory database and
need no external server. Run from the repository with Rust 1.94 or later:

```sh
cargo +1.94.0 run --no-default-features --features sqlite,runtime-tokio --example crud
```

| Example | Demonstrates |
| --- | --- |
| `crud` | Insert, typed filters, update and delete |
| `relations` | Declare and eagerly load a has-many relation using its constant |
| `transactions` | Commit a closure; return `Err` to roll back |
| `migrations` | Register and run a reversible schema migration |
| `scopes` | Declare a reusable typed query scope |
| `postgres` | Connect using `DATABASE_URL` and run a read-only query |

Replace `crud` with the other SQLite example names in the command above. For
PostgreSQL, supply a URL for a running server:

```sh
DATABASE_URL=postgres://user:password@localhost/database cargo +1.94.0 run --example postgres
```

Compile all examples without a server:

```sh
cargo +1.94.0 check --all-features --examples
```

See [API.md](../API.md) for signatures, common patterns and feature requirements.
