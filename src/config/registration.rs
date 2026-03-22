use crate::migration::Migration;
use crate::seeding::Seed;

pub trait RegisterMigrations {
    fn collect() -> Vec<Box<dyn Migration>>;
}

impl RegisterMigrations for () {
    fn collect() -> Vec<Box<dyn Migration>> {
        Vec::new()
    }
}

pub trait RegisterSeeds {
    fn collect() -> Vec<Box<dyn Seed>>;
}

impl RegisterSeeds for () {
    fn collect() -> Vec<Box<dyn Seed>> {
        Vec::new()
    }
}

macro_rules! impl_register_tuples {
    ($first:ident) => {
        impl<$first: Migration + Default + 'static> RegisterMigrations for ($first,) {
            fn collect() -> Vec<Box<dyn Migration>> {
                vec![Box::new($first::default())]
            }
        }
        impl<$first: Seed + Default + 'static> RegisterSeeds for ($first,) {
            fn collect() -> Vec<Box<dyn Seed>> {
                vec![Box::new($first::default())]
            }
        }
    };
    ($first:ident, $($rest:ident),+) => {
        impl_register_tuples!($($rest),+);

        impl<$first: Migration + Default + 'static, $($rest: Migration + Default + 'static),+>
            RegisterMigrations for ($first, $($rest),+)
        {
            fn collect() -> Vec<Box<dyn Migration>> {
                vec![Box::new($first::default()), $(Box::new($rest::default())),+]
            }
        }

        impl<$first: Seed + Default + 'static, $($rest: Seed + Default + 'static),+>
            RegisterSeeds for ($first, $($rest),+)
        {
            fn collect() -> Vec<Box<dyn Seed>> {
                vec![Box::new($first::default()), $(Box::new($rest::default())),+]
            }
        }
    };
}

impl_register_tuples!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21,
    T22, T23, T24, T25, T26, T27, T28, T29, T30, T31, T32, T33, T34, T35, T36, T37, T38, T39, T40,
    T41, T42, T43, T44, T45, T46, T47, T48, T49, T50, T51, T52, T53, T54, T55, T56, T57, T58, T59,
    T60, T61, T62, T63, T64, T65, T66, T67, T68, T69, T70, T71, T72, T73, T74, T75, T76, T77, T78,
    T79, T80, T81, T82, T83, T84, T85, T86, T87, T88, T89, T90, T91, T92, T93, T94, T95, T96, T97,
    T98, T99, T100, T101, T102, T103, T104, T105, T106, T107, T108, T109, T110, T111, T112, T113,
    T114, T115, T116, T117, T118, T119, T120, T121, T122, T123, T124, T125, T126, T127, T128, T129,
    T130, T131, T132, T133, T134, T135, T136, T137, T138, T139, T140, T141, T142, T143, T144, T145,
    T146, T147, T148, T149, T150, T151, T152, T153, T154, T155, T156, T157, T158, T159, T160, T161,
    T162, T163, T164, T165, T166, T167, T168, T169, T170, T171, T172, T173, T174, T175, T176, T177,
    T178, T179, T180, T181, T182, T183, T184, T185, T186, T187, T188, T189, T190, T191, T192, T193,
    T194, T195, T196, T197, T198, T199, T200
);
