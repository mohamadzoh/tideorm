-- TideORM Generated Schema
-- Database: Postgres
-- Generated at: 2026-01-10 22:02:52 UTC

CREATE TABLE IF NOT EXISTS "users" (
    "id" BIGSERIAL,
    "email" VARCHAR(255) NOT NULL,
    "name" VARCHAR(255) NOT NULL,
    "password_hash" TEXT NOT NULL,
    "status" VARCHAR(50) NOT NULL DEFAULT 'active',
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "posts" (
    "id" BIGSERIAL,
    "author_id" BIGINT NOT NULL,
    "slug" VARCHAR(255) NOT NULL,
    "title" VARCHAR(255) NOT NULL,
    "content" TEXT,
    "status" VARCHAR(50) NOT NULL DEFAULT 'draft',
    "published_at" TIMESTAMPTZ,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "updated_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    "deleted_at" TIMESTAMPTZ,
    PRIMARY KEY ("id")
);

CREATE TABLE IF NOT EXISTS "categories" (
    "id" BIGSERIAL,
    "parent_id" BIGINT,
    "name" VARCHAR(255) NOT NULL,
    "slug" VARCHAR(255) NOT NULL,
    "description" TEXT,
    "sort_order" INTEGER NOT NULL DEFAULT 0,
    "created_at" TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY ("id")
);

CREATE INDEX IF NOT EXISTS "idx_users_email" ON "users" ("email");
CREATE INDEX IF NOT EXISTS "idx_users_status" ON "users" ("status");
CREATE UNIQUE INDEX IF NOT EXISTS "uidx_users_email" ON "users" ("email");

CREATE INDEX IF NOT EXISTS "idx_posts_author_id" ON "posts" ("author_id");
CREATE INDEX IF NOT EXISTS "idx_posts_status_published" ON "posts" ("status", "published_at");
CREATE UNIQUE INDEX IF NOT EXISTS "uidx_posts_slug" ON "posts" ("slug");

CREATE INDEX IF NOT EXISTS "idx_categories_parent_id" ON "categories" ("parent_id");
CREATE UNIQUE INDEX IF NOT EXISTS "uidx_categories_slug" ON "categories" ("slug");

