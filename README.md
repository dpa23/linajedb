# linaje.db

Multi-engine terminal database client with row lineage tracing: select any
row and inspect its full ancestry (parents of parents) and descent (children
of children) across foreign keys, inferred references, or graph edges. The
lineage is available as an interactive tree, as JSON, and through a headless
CLI designed for scripts and AI agents.

Supported engines: MariaDB/MySQL, PostgreSQL, SQLite, MongoDB, Neo4j, and
local JSON/BSON/DBF files.

## Motivation

Answering "where does this row come from and what depends on it?" normally
requires walking `INFORMATION_SCHEMA` by hand and running one query per
foreign key. linaje.db performs the complete walk in a single step, in both
directions, with cycle detection and bounded depth.

```
● borrador  id_borrador=1, descripcion=PAGO 1ERA QUINCENA..., id_tipo=4, ...
├─▲ empresa (borrador.id_empresa = empresa.idEmpresa)  idEmpresa=1, Nombre=Inopcon, ...
├─▲ centro_costo (borrador.id_centro_costo = centro_costo.id_centro_costo)  ...
│  └─▲ centro_clasificacion (...)  clasificacion=Mano de Obra Directa, ...
│     └─▲ centro_tipo (...)  centro_tipo=Egresos
├─▲ proyectos (borrador.id_proyectos = proyectos.idProyectos)  Sede=Santa Elena, ...
│  └─▲ empresa (...)  [cycle: already traced]
└─▼ tesoreria (tesoreria.id_borrador = borrador.id_borrador)  ...
```

## Architecture

The UI never blocks on the database: a worker task owns every connection and
communicates with the interface through channels. The same worker serves the
TUI and the headless CLI.

```mermaid
flowchart LR
    subgraph Interface
        TUI[TUI event loop - main.rs / ui.rs]
        CLI[Headless CLI - headless.rs]
    end
    subgraph Worker["DbWorker (db/mod.rs)"]
        DISPATCH[Request dispatch]
        WALK[Lineage walkers: relational FKs, Mongo conventions, Neo4j edges]
    end
    subgraph Engines
        MY[(MariaDB/MySQL)]
        PG[(PostgreSQL)]
        SQ[(SQLite)]
        MO[(MongoDB)]
        NEO[(Neo4j)]
    end
    TUI -- DbRequest --> DISPATCH
    CLI -- DbRequest --> DISPATCH
    DISPATCH --> WALK
    WALK --> MY & PG & SQ & MO & NEO
    DISPATCH -- DbResponse --> TUI
    DISPATCH -- DbResponse --> CLI
```

## Installation

Requires the Rust toolchain (https://rustup.rs). No libraries need to be
installed on the system: the database drivers are Rust crates and the only C
dependency (bundled SQLite) is compiled automatically, so a plain
`cargo build` is the whole process on every platform.

### Linux

```bash
git clone https://github.com/dpa23/linajedb.git
cd linajedb
cargo build --release
# binary: target/release/linajedb
```

### Windows

Install Rust with rustup (the default MSVC toolchain; it requires the Visual
Studio Build Tools installer that rustup points to). Then, from any terminal:

```
git clone https://github.com/dpa23/linajedb.git
cd linajedb
cargo build --release
# binary: target\release\linajedb.exe
```

The interface uses Unicode box-drawing and geometric characters, so use a
modern terminal with a font that covers them (any Nerd Font or Cascadia
Code):

- Windows: WezTerm or Alacritty are recommended; Windows Terminal also
  works. The legacy `cmd.exe` console host is not supported.
- Linux: any modern emulator (WezTerm, Kitty, Alacritty, GNOME Terminal).

Minimum terminal size: 80x24. The layout is proportional and adapts to any
size above that; below it, the client shows a resize notice instead of
rendering a broken layout.

## TUI usage

```bash
cargo run            # or target/release/linajedb
```

The connection screen offers three modes: discovered profiles (reads
`~/.my.cnf` and `~/.pgpass` when present), a manual form, and a raw
connection URL. Select the engine with Left/Right.

### Finding things

| Scope | How |
|---|---|
| Databases | `d` opens the database list; `/` filters it as you type |
| Tables / collections / labels | `/` in the sidebar filters the list; Enter opens the selection |
| Rows | `/` in the data grid filters rows client-side (substring, all columns) |
| Columns | Left/Right move the cell cursor; the title shows `col N/M (name)` |

### Key bindings (data grid)

| Key | Action |
|---|---|
| `t` | Trace the selected row's full lineage; `j` toggles tree/JSON view |
| `Enter` | Record view: the row as a column/value list with complete values |
| `Left`/`Right`, `Home`/`End` | Move the cell cursor; columns auto-scroll |
| `/` | Filter rows in the grid |
| `i` | Describe table: columns, types, primary/foreign key roles |
| `e` / `a` / `d` | Edit / add / delete the selected row |
| `r` | Re-run the current query |
| `F6` / `F7` | Chart (BI pivot) mode / related-data split |
| `Tab` | Cycle focus between panes; the toolbar is clickable |

## Headless trace (scripts and AI agents)

```bash
linajedb trace --url mysql://user:pass@host:3306/shop \
    --table orders --where "id_order=118" --format tree

linajedb trace --url mongodb://localhost:27017/app \
    --table users --where '{"email": "ana@example.com"}'    # JSON output

linajedb trace --url bolt://neo4j:password@localhost:7687 \
    --table Person --where "name=Alice"
```

JSON is written to stdout (pipe it to `jq` or feed it to a language model);
errors go to stderr with exit code 1.

Lineage resolution per engine:

| Engine | Parents | Children |
|---|---|---|
| Relational | declared foreign keys | tables whose foreign keys reference the row |
| MongoDB | fields named `x_id` / `id_x` / `xId` resolved to collection `x(s)`, with ObjectId and hex-string cross-matching | sibling collections holding such a field equal to the document's `_id` |
| Neo4j | outgoing relationships | incoming relationships |

Bounds: 4 ancestor levels, 3 descendant levels, 5 rows per relation, 200
nodes in total. Cycles are detected and annotated instead of re-expanded.

## License

MIT. See `LICENSE`.
