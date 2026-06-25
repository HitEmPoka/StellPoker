#!/usr/bin/env npx tsx
/**
 * Generates TypeScript interfaces from the coordinator PostgreSQL schema.
 *
 * Usage:
 *   DATABASE_URL=postgres://coordinator:pass@localhost/coordinator npx tsx scripts/db/generate-types.ts
 *
 * Output: app/src/types/db.generated.ts
 *
 * Dependencies (install once):  npm install --save-dev tsx pg @types/pg
 */

import { Client } from "pg";
import { writeFileSync, mkdirSync } from "fs";
import { dirname } from "path";

const OUTPUT_PATH = "app/src/types/db.generated.ts";

// PostgreSQL -> TypeScript type map
const PG_TO_TS: Record<string, string> = {
  integer: "number",
  bigint: "string", // bigint comes as string from pg driver
  bigserial: "string",
  serial: "number",
  text: "string",
  boolean: "boolean",
  "timestamp with time zone": "string", // ISO 8601 string
  "timestamp without time zone": "string",
  uuid: "string",
  "character varying": "string",
  "ARRAY": "unknown[]", // refined per column below
  json: "unknown",
  jsonb: "unknown",
};

// PostgreSQL array element type map (udt_name for array columns)
const PG_ARRAY_ELEM: Record<string, string> = {
  _text: "string",
  _int4: "number",
  _int8: "string",
  _uuid: "string",
  _bool: "boolean",
};

function toCamelCase(snake: string): string {
  return snake.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

function toPascalCase(snake: string): string {
  const camel = toCamelCase(snake);
  return camel.charAt(0).toUpperCase() + camel.slice(1);
}

interface ColumnInfo {
  tableName: string;
  columnName: string;
  dataType: string;
  udtName: string;
  isNullable: string;
  columnDefault: string | null;
}

async function generate() {
  const url = process.env.DATABASE_URL;
  if (!url) {
    console.error("ERROR: DATABASE_URL environment variable is not set.");
    process.exit(1);
  }

  const client = new Client({ connectionString: url });
  await client.connect();

  const { rows } = await client.query<ColumnInfo>(`
    SELECT
      table_name    AS "tableName",
      column_name   AS "columnName",
      data_type     AS "dataType",
      udt_name      AS "udtName",
      is_nullable   AS "isNullable",
      column_default AS "columnDefault"
    FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name NOT IN ('_sqlx_migrations')
    ORDER BY table_name, ordinal_position
  `);

  await client.end();

  // Group columns by table
  const tables = new Map<string, ColumnInfo[]>();
  for (const row of rows) {
    const cols = tables.get(row.tableName) ?? [];
    cols.push(row);
    tables.set(row.tableName, cols);
  }

  const lines: string[] = [
    "// AUTO-GENERATED — do not edit by hand.",
    `// Source: coordinator PostgreSQL schema`,
    `// Generated: ${new Date().toISOString()}`,
    `// Run: DATABASE_URL=... npx tsx scripts/db/generate-types.ts`,
    "",
  ];

  for (const [tableName, columns] of tables) {
    lines.push(`export interface ${toPascalCase(tableName)} {`);
    for (const col of columns) {
      let tsType: string;
      if (col.dataType === "ARRAY") {
        const elem = PG_ARRAY_ELEM[col.udtName] ?? "unknown";
        tsType = `${elem}[]`;
      } else {
        tsType = PG_TO_TS[col.dataType] ?? "unknown";
      }
      const optional = col.isNullable === "YES" || col.columnDefault !== null ? "?" : "";
      lines.push(`  ${toCamelCase(col.columnName)}${optional}: ${tsType} | null;`);
    }
    lines.push("}");
    lines.push("");
    // Also export an insert type (omit auto-generated columns)
    const insertCols = columns.filter(
      (c) =>
        !["id", "created_at", "updated_at"].includes(c.columnName) &&
        c.columnDefault === null
    );
    if (insertCols.length > 0) {
      lines.push(`export interface New${toPascalCase(tableName)} {`);
      for (const col of insertCols) {
        let tsType: string;
        if (col.dataType === "ARRAY") {
          const elem = PG_ARRAY_ELEM[col.udtName] ?? "unknown";
          tsType = `${elem}[]`;
        } else {
          tsType = PG_TO_TS[col.dataType] ?? "unknown";
        }
        const optional = col.isNullable === "YES" ? "?" : "";
        lines.push(`  ${toCamelCase(col.columnName)}${optional}: ${tsType};`);
      }
      lines.push("}");
      lines.push("");
    }
  }

  mkdirSync(dirname(OUTPUT_PATH), { recursive: true });
  writeFileSync(OUTPUT_PATH, lines.join("\n"), "utf8");
  console.log(`Types written to ${OUTPUT_PATH}`);
}

generate().catch((err) => {
  console.error(err);
  process.exit(1);
});
