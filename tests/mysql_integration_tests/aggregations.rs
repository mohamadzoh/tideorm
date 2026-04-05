use super::*;

pub(super) async fn run() {
    // =========================================================================
    // AGGREGATION TESTS
    // =========================================================================
    println!("📊 Testing: Aggregations");
    {
        let sum = TestUser::query().sum("age").await.expect("Sum failed");
        // Ages: 21+22+23+24+25+26+27+28+29+30 = 255
        assert_eq!(sum as i64, 255);
        println!("   ✓ sum works");

        let avg = TestUser::query().avg("age").await.expect("Avg failed");
        assert!((avg - 25.5).abs() < 0.01);
        println!("   ✓ avg works");

        let min = TestUser::query().min("age").await.expect("Min failed");
        assert_eq!(min as i64, 21);
        println!("   ✓ min works");

        let max = TestUser::query().max("age").await.expect("Max failed");
        assert_eq!(max as i64, 30);
        println!("   ✓ max works");
    }
    println!();
}
