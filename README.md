# linaje.db

Multi-engine terminal database client with **row lineage tracing**: pick any
row and see its full ancestry (parents of parents) and descent (children of
children) across foreign keys, inferred references, or graph edges — as an
interactive tree, JSON, or from a headless CLI built for scripts and AI agents.

Supports **MariaDB/MySQL, PostgreSQL, SQLite, MongoDB, Neo4j** and local
JSON/BSON/DBF files.

## Why

Answering "where does this row come from and what hangs off it?" normally
means hand-walking `INFORMATION_SCHEMA` and running a query per foreign key.
`linajedb` does the whole walk in one step, in both directions, with cycle
detection and sane limits.

```
● borrador  id_borrador=1, descripcion=PAGO 1ERA QUINCENA…, id_tipo=4, …
├─▲ empresa (borrador.id_empresa = empresa.idEmpresa)  idEmpresa=1, Nombre=Inopcon, …
├─▲ centro_costo (borrador.id_centro_costo = centro_costo.id_centro_costo)  …
│  └─▲ centro_clasificacion (…)  clasificacion=Mano de Obra Directa, …
│     └─▲ centro_tipo (…)  centro_tipo=Egresos
├─▲ proyectos (borrador.id_proyectos = proyectos.idProyectos)  Sede=Santa Elena, …
│  └─▲ empresa (…)  [cycle: already traced]
└─▼ tesoreria (tesoreria.id_borrador = borrador.id_borrador)  …
```

## TUI

```bash
cargo run
```

A lazysql-style client: connection profiles (auto-discovers `~/.my.cnf` /
`~/.pgpass`), clickable toolbar, always-on query bar, in-grid search,
content-sized columns with a cell cursor, BI pivot charts, and parent/child
split views.

Key bindings in the data grid:

| Key | Action |
|---|---|
| `t` | **Trace** the selected row's full lineage (tree ⇄ JSON with `j`) |
| `Enter` | Record view: the row as a column/value list with full values |
| `←/→` `Home/End` | Move the cell cursor (auto-scrolls columns) |
| `/` | Filter rows in-grid |
| `i` | Describe table (columns, types, PK/FK roles) |
| `e` / `a` / `d` | Edit / add / delete row |
| `F6` / `F7` | BI chart mode / related-data split |

## Headless trace (for scripts & AI agents)

```bash
linajedb trace --url mysql://user:pass@host:3306/shop \
    --table orders --where "id_order=118" --format tree

linajedb trace --url mongodb://localhost:27017/app \
    --table users --where '{"email": "ana@example.com"}'   # JSON output

linajedb trace --url bolt://neo4j:password@localhost:7687 \
    --table Person --where "name=Alice"
```

JSON goes to stdout (pipe it to `jq` or feed it to an LLM), errors to stderr
with exit code 1.

How lineage is resolved per engine:

- **Relational** — declared foreign keys, both directions.
- **MongoDB** — references inferred by naming convention
  (`user_id` / `id_user` / `userId` → collection `user(s)`), with
  ObjectId ⇄ hex-string cross-matching.
- **Neo4j** — real edges: outgoing = parents, incoming = children.

Bounds: 4 ancestor levels, 3 descendant levels, 5 rows per relation,
200 nodes total; cycles are detected and marked.

## Build

```bash
cargo build --release   # binary: target/release/linajedb
```
