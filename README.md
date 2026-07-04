# linaje.db

Cliente de bases de datos para terminal, multi-motor, con trazado de linaje
de filas: selecciona cualquier fila e inspecciona toda su ascendencia (padres
de los padres) y descendencia (hijas de las hijas) a través de llaves
foráneas, referencias inferidas o aristas de grafo. El linaje está disponible
como árbol interactivo, como JSON y mediante una CLI headless diseñada para
scripts y agentes de IA.

Motores soportados: MariaDB/MySQL, PostgreSQL, SQLite, MongoDB, Neo4j y
archivos locales JSON/BSON/DBF.

## Motivación

Responder "¿de dónde viene esta fila y qué depende de ella?" normalmente
exige recorrer `INFORMATION_SCHEMA` a mano y ejecutar una consulta por cada
llave foránea. linaje.db realiza el recorrido completo en un solo paso, en
ambas direcciones, con detección de ciclos y profundidad acotada.

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

## Arquitectura

La interfaz nunca se bloquea esperando a la base de datos: una tarea worker
es dueña de cada conexión y se comunica con la interfaz por canales. El mismo
worker atiende al TUI y a la CLI headless.

```mermaid
flowchart LR
    subgraph Interfaz
        TUI[Bucle de eventos del TUI - main.rs / ui.rs]
        CLI[CLI headless - headless.rs]
    end
    subgraph Worker["DbWorker (db/mod.rs)"]
        DISPATCH[Despacho de solicitudes]
        WALK[Recorredores de linaje: FKs relacionales, convenciones Mongo, aristas Neo4j]
    end
    subgraph Motores
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

## Descarga

Tres formas de obtener el proyecto:

1. Con git:

   ```bash
   git clone https://github.com/dpa23/linajedb.git
   ```

2. Sin git: en https://github.com/dpa23/linajedb usar el botón
   "Code" > "Download ZIP" y descomprimir.

3. Compilar e instalar directo desde GitHub en un solo comando (deja el
   binario `linajedb` en `~/.cargo/bin`, ya incluido en el PATH por rustup):

   ```bash
   cargo install --git https://github.com/dpa23/linajedb.git
   ```

## Instalación

Requiere la toolchain de Rust (https://rustup.rs). No hace falta instalar
librerías en el sistema: los drivers de base de datos son crates de Rust y la
única dependencia en C (SQLite embebido) se compila automáticamente, así que
un simple `cargo build` es todo el proceso en cualquier plataforma.

### Linux

```bash
git clone https://github.com/dpa23/linajedb.git
cd linajedb
cargo build --release
# binario: target/release/linajedb
# opcional: instalarlo en el PATH del usuario (~/.cargo/bin)
cargo install --path .
```

### Windows

Instala Rust con rustup (la toolchain MSVC por defecto; requiere las Visual
Studio Build Tools, cuyo instalador el propio rustup indica). Luego, desde
cualquier terminal:

```
git clone https://github.com/dpa23/linajedb.git
cd linajedb
cargo build --release
# binario: target\release\linajedb.exe
```

La interfaz usa caracteres Unicode de cajas y geométricos, así que conviene
una terminal moderna con una fuente que los cubra (cualquier Nerd Font o
Cascadia Code):

- Windows: se recomienda WezTerm o Alacritty; Windows Terminal también
  funciona. La consola clásica `cmd.exe` no está soportada.
- Linux: cualquier emulador moderno (WezTerm, Kitty, Alacritty, GNOME
  Terminal).

Tamaño mínimo de terminal: 80x24. El layout es proporcional y se adapta a
cualquier tamaño por encima de ese; por debajo, el cliente muestra un aviso
para redimensionar en lugar de renderizar un layout roto.

## Uso del TUI

```bash
cargo run            # o target/release/linajedb
```

La pantalla de conexión ofrece tres modos: perfiles descubiertos (lee
`~/.my.cnf` y `~/.pgpass` cuando existen), un formulario manual y una URL de
conexión directa. El motor se elige con Izquierda/Derecha.

### Cómo buscar

| Ámbito | Cómo |
|---|---|
| Bases de datos | `d` abre la lista de bases; `/` la filtra mientras escribes |
| Tablas / colecciones / labels | `/` en el panel lateral filtra la lista; Enter abre la selección |
| Filas | `/` en la grilla filtra las filas en el cliente (subcadena, todas las columnas) |
| Columnas | Izquierda/Derecha mueven el cursor de celda; el título muestra `col N/M (nombre)` |

### Atajos de teclado (grilla de datos)

| Tecla | Acción |
|---|---|
| `t` | Traza el linaje completo de la fila seleccionada; `j` alterna árbol/JSON |
| `Enter` | Vista de registro: la fila como lista columna/valor con valores completos |
| `Izq`/`Der`, `Inicio`/`Fin` | Mueven el cursor de celda; las columnas se desplazan solas |
| `/` | Filtra filas en la grilla |
| `i` | Describe la tabla: columnas, tipos, roles de llave primaria/foránea |
| `e` / `a` / `d` | Editar / agregar / eliminar la fila seleccionada |
| `r` | Re-ejecuta la consulta actual |
| `F6` / `F7` | Modo gráfico (pivote BI) / panel de datos relacionados |
| `Tab` | Cicla el foco entre paneles; la barra de acciones acepta clics |

## Trazado headless (scripts y agentes de IA)

```bash
linajedb trace --url mysql://usuario:clave@host:3306/tienda \
    --table ordenes --where "id_orden=118" --format tree

linajedb trace --url mongodb://localhost:27017/app \
    --table usuarios --where '{"correo": "ana@ejemplo.com"}'    # salida JSON

linajedb trace --url bolt://neo4j:clave@localhost:7687 \
    --table Persona --where "nombre=Alicia"
```

El JSON se escribe a stdout (se puede canalizar a `jq` o entregar a un modelo
de lenguaje); los errores van a stderr con código de salida 1.

Resolución del linaje según el motor:

| Motor | Padres | Hijas |
|---|---|---|
| Relacionales | llaves foráneas declaradas | tablas cuyas llaves foráneas referencian la fila |
| MongoDB | campos con nombre `x_id` / `id_x` / `xId` resueltos a la colección `x(s)`, con cruce ObjectId y cadena hexadecimal | colecciones hermanas que tengan uno de esos campos igual al `_id` del documento |
| Neo4j | relaciones salientes | relaciones entrantes |

Límites: 4 niveles de ancestros, 3 de descendientes, 5 filas por relación,
200 nodos en total. Los ciclos se detectan y se anotan en lugar de volver a
expandirse.

## Licencia

MIT. Ver `LICENSE`.
