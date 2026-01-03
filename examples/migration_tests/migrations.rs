//! Test Migrations
//!
//! This file contains sample migrations used for testing the migration system.

use tideorm::prelude::*;

// ============================================================================
// MIGRATION 1: Create Products Table
// ============================================================================

#[derive(Default)]
pub struct CreateProductsTable;

#[async_trait]
impl Migration for CreateProductsTable {
    fn version(&self) -> &str {
        "20260106_001"
    }

    fn name(&self) -> &str {
        "create_products_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("test_products", |t| {
                t.id();
                t.string("name").not_null();
                t.string("sku").unique().not_null();
                t.decimal("price").not_null();
                t.integer("quantity").default(0);
                t.boolean("active").default(true);
                t.timestamps();

                t.index(&["active"]);
                t.index(&["sku"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("test_products").await
    }
}

// ============================================================================
// MIGRATION 2: Create Orders Table
// ============================================================================

#[derive(Default)]
pub struct CreateOrdersTable;

#[async_trait]
impl Migration for CreateOrdersTable {
    fn version(&self) -> &str {
        "20260106_002"
    }

    fn name(&self) -> &str {
        "create_orders_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("test_orders", |t| {
                t.id();
                t.string("order_number").unique().not_null();
                t.string("status").default("pending").not_null();
                t.decimal("total").default(0);
                t.text("notes");
                t.timestamps();

                t.index(&["status"]);
                t.index(&["order_number"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("test_orders").await
    }
}

// ============================================================================
// MIGRATION 3: Create Order Items Table
// ============================================================================

#[derive(Default)]
pub struct CreateOrderItemsTable;

#[async_trait]
impl Migration for CreateOrderItemsTable {
    fn version(&self) -> &str {
        "20260106_003"
    }

    fn name(&self) -> &str {
        "create_order_items_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("test_order_items", |t| {
                t.id();
                t.foreign_id("order_id").not_null();
                t.foreign_id("product_id").not_null();
                t.integer("quantity").default(1).not_null();
                t.decimal("unit_price").not_null();
                t.decimal("subtotal").not_null();
                t.timestamps();

                t.index(&["order_id"]);
                t.index(&["product_id"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("test_order_items").await
    }
}

// ============================================================================
// MIGRATION 4: Add Description to Products
// ============================================================================

#[derive(Default)]
pub struct AddDescriptionToProducts;

#[async_trait]
impl Migration for AddDescriptionToProducts {
    fn version(&self) -> &str {
        "20260106_004"
    }

    fn name(&self) -> &str {
        "add_description_to_products"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .alter_table("test_products", |t| {
                t.add_column("description", ColumnType::Text).nullable();
                t.add_column("category", ColumnType::String).nullable();
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .alter_table("test_products", |t| {
                t.drop_column("description");
                t.drop_column("category");
            })
            .await
    }
}

// ============================================================================
// MIGRATION 5: Create Inventory Table (for testing complex scenarios)
// ============================================================================

#[derive(Default)]
pub struct CreateInventoryTable;

#[async_trait]
impl Migration for CreateInventoryTable {
    fn version(&self) -> &str {
        "20260106_005"
    }

    fn name(&self) -> &str {
        "create_inventory_table"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema
            .create_table("test_inventory", |t| {
                t.id();
                t.foreign_id("product_id").not_null();
                t.string("warehouse").not_null();
                t.integer("quantity").default(0).not_null();
                t.integer("reserved").default(0);
                t.datetime("last_restock");
                t.timestamps();

                // Composite unique index
                t.unique_index(&["product_id", "warehouse"]);
            })
            .await
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table("test_inventory").await
    }
}
