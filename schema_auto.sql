-- TideORM Generated Schema
-- Database: Postgres
-- Generated at: 2026-01-10 22:02:52 UTC

CREATE TABLE IF NOT EXISTS ""public"."posts"" (
    "id" BIGSERIAL,
    "user_id" BIGINT NOT NULL,
    "title" TEXT NOT NULL,
    "body" TEXT NOT NULL,
    "published" BOOLEAN NOT NULL,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL,
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS ""public"."users"" (
    "id" BIGSERIAL,
    "email" TEXT NOT NULL,
    "name" TEXT NOT NULL,
    "bio" TEXT,
    "active" BOOLEAN NOT NULL,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL,
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "_migrations" (
    "id" BIGSERIAL,
    "version" CHARACTER VARYING NOT NULL,
    "name" CHARACTER VARYING NOT NULL,
    "applied_at" TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "bench_products" (
    "id" BIGSERIAL,
    "name" CHARACTER VARYING NOT NULL,
    "category" CHARACTER VARYING NOT NULL,
    "price" INTEGER NOT NULL,
    "stock" INTEGER NOT NULL,
    "active" BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "bench_users" (
    "id" BIGSERIAL,
    "email" CHARACTER VARYING NOT NULL,
    "name" CHARACTER VARYING NOT NULL,
    "age" INTEGER NOT NULL,
    "active" BOOLEAN NOT NULL DEFAULT true,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "comments" (
    "id" BIGSERIAL,
    "post_id" BIGINT NOT NULL,
    "user_id" BIGINT NOT NULL,
    "content" TEXT NOT NULL,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "customer_orders" (
    "id" BIGSERIAL,
    "order_number" TEXT NOT NULL,
    "user_id" BIGINT NOT NULL,
    "status" TEXT NOT NULL,
    "total" BIGINT NOT NULL,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "posts" (
    "id" BIGSERIAL,
    "author_id" BIGINT NOT NULL,
    "slug" CHARACTER VARYING NOT NULL,
    "status" CHARACTER VARYING NOT NULL DEFAULT 'draft'::character varying,
    "view_count" INTEGER NOT NULL DEFAULT 0,
    "title" CHARACTER VARYING NOT NULL,
    "content" TEXT NOT NULL,
    "excerpt" TEXT,
    "published_at" TIMESTAMP WITH TIME ZONE,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    "deleted_at" TIMESTAMP WITH TIME ZONE,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "products" (
    "id" BIGSERIAL,
    "name" CHARACTER VARYING NOT NULL,
    "category" CHARACTER VARYING NOT NULL,
    "price" BIGINT NOT NULL,
    "stock" INTEGER NOT NULL DEFAULT 0,
    "active" BOOLEAN NOT NULL DEFAULT true,
    "attributes" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "related_skus" ARRAY NOT NULL DEFAULT '{}'::text[],
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "profiles" (
    "id" BIGSERIAL,
    "user_id" BIGINT NOT NULL,
    "bio" TEXT,
    "website" CHARACTER VARYING,
    "settings" JSONB NOT NULL DEFAULT '{}'::jsonb,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "settings" (
    "id" BIGSERIAL,
    "tenant_id" BIGINT NOT NULL,
    "key" CHARACTER VARYING NOT NULL,
    "value" TEXT NOT NULL,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "users" (
    "id" BIGSERIAL,
    "email" CHARACTER VARYING NOT NULL,
    "name" CHARACTER VARYING NOT NULL,
    "status" CHARACTER VARYING NOT NULL DEFAULT 'active'::character varying,
    "created_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    "updated_at" TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT now(),
    PRIMARY KEY ("id")
);

CREATE UNIQUE INDEX IF NOT EXISTS "_migrations_version_key" ON "_migrations" ("version");

CREATE INDEX IF NOT EXISTS "idx_bench_products_active" ON "bench_products" ("active");
CREATE INDEX IF NOT EXISTS "idx_bench_products_category" ON "bench_products" ("category");
CREATE INDEX IF NOT EXISTS "idx_bench_products_price" ON "bench_products" ("price");

CREATE INDEX IF NOT EXISTS "idx_posts_slug" ON "posts" ("slug");
CREATE UNIQUE INDEX IF NOT EXISTS "posts_slug_key" ON "posts" ("slug");
CREATE INDEX IF NOT EXISTS "idx_posts_author_id" ON "posts" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_posts_deleted_at" ON "posts" ("deleted_at");
CREATE INDEX IF NOT EXISTS "idx_posts_status" ON "posts" ("status");

CREATE INDEX IF NOT EXISTS "idx_products_category" ON "products" ("category");
CREATE INDEX IF NOT EXISTS "idx_products_attributes" ON "products" ("attributes");

CREATE UNIQUE INDEX IF NOT EXISTS "profiles_user_id_key" ON "profiles" ("user_id");

CREATE UNIQUE INDEX IF NOT EXISTS "settings_tenant_id_key_key" ON "settings" ("tenant_id", "key");

CREATE UNIQUE INDEX IF NOT EXISTS "users_email_key" ON "users" ("email");

