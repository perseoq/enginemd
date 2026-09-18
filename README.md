# EngineMD

Servidor de Markdown renderizado a HTML. Convierte archivos `.md` en páginas HTML al estilo GitHub Pages, con soporte para MathJax, Mermaid, Chart.js y más.

## Instalación

```bash
git clone <repo>
cd enginemd
cargo build --release
./target/release/enginemd fetch   # Descarga dependencias JS
```

## Uso rápido

```bash
# Crear un proyecto nuevo
enginemd new mi-docs

# Iniciar servidor (http://0.0.0.0:10300)
enginemd

# Servidor con hot-reload (http://0.0.0.0:9696)
enginemd --watch
```

## Comandos

| Comando | Descripción |
|---------|-------------|
| `enginemd new <name>` | Crea un sitio nuevo en `~/.enginemd/sites/<name>/` |
| `enginemd up <path>` | Registra un directorio existente como sitio |
| `enginemd down <name>` | Elimina un sitio del listado |
| `enginemd fetch [--force]` | Descarga las librerías JS/CSS al caché local |
| `enginemd` | Inicia servidor en `0.0.0.0:10300` |
| `enginemd --watch` | Inicia servidor dev en `0.0.0.0:9696` con hot-reload |
| `enginemd daemon start` | Inicia el servidor en segundo plano (persiste al reiniciar) |
| `enginemd daemon stop` | Detiene el daemon y desactiva el autoarranque |
| `enginemd daemon restart` | Reinicia el daemon |
| `enginemd daemon status` | Muestra el estado del daemon |

## Flags del servidor

```bash
enginemd --path /ruta               # Sirve un solo directorio
enginemd --port 8080                # Puerto personalizado
enginemd --js-support mathjax,mermaid,chartjs  # Librerías JS (allowlist)
enginemd --css-support dark         # Tema CSS
enginemd --lang es                  # Idioma
```

## Assets JS/CSS (self-hosted)

Todas las librerías se **descargan y se sirven localmente**; la página nunca
carga desde un CDN. Al arrancar, `auto_fetch` descarga lo que falte en segundo
plano (y también se descarga bajo demanda si falta). Los archivos se guardan en
`~/.enginemd/js` y `~/.enginemd/css`, con un manifest de hashes SRI en
`~/.enginemd/assets.json`.

- **Detección automática**: cada página carga solo lo que usa (```mermaid,
  ```chart, `$...$`, etc.), aunque no esté en `--js-support`.
- `--js-support` es una **allowlist** de claves del catálogo (override global).
- **SRI** activo por defecto (`sri: false` para desactivarlo) y `defer`.
- Si un asset no está y no se puede descargar, se **avisa y se omite**.
- `enginemd fetch --force` fuerza la re-descarga.

Catálogo actual: `mathjax`, `katex`, `mermaid`, `chartjs`, `highlight`, `anchor`,
`fontawesome`. El resaltado de código es server-side (syntect) por defecto.

## Modo daemon

Ejecuta el servidor en segundo plano y lo deja corriendo tras reiniciar la
máquina, sin configurar un servicio:

```bash
enginemd daemon start     # arranca en segundo plano + autoarranque @reboot
enginemd daemon status    # estado y autoarranque
enginemd daemon stop      # detiene y desactiva el autoarranque
enginemd daemon restart
```

Los flags globales (`--port`, `--path`, `--lang`, ...) se reenvían
al daemon. PID y log en `~/.enginemd/enginemd.pid` y `~/.enginemd/enginemd.log`.
Ver `MANUAL_DAEMON.md`.

## Páginas

- `index.md` o `init.md` → página principal del sitio
- `about.md` → `/about`
- `docs/guide.md` → `/docs/guide`

Si el archivo no tiene frontmatter con `title`, se usa el primer
`# Heading` del Markdown como título.

## Librerías JS soportadas

| Flag | Librería |
|------|----------|
| `mathjax` | MathJax 3 — renderizado de $\LaTeX$ |
| `mermaid` | Mermaid — diagramas desde texto |
| `chartjs` | Chart.js — gráficos desde JSON |
| `highlight` | Highlight.js — resaltado de código |
| `katex` | KaTeX — MathJax alternativo (más rápido) |
| `fontawesome` | Font Awesome 6 — iconos |
| `anchor` | AnchorJS — enlaces anchor en headings |

### Chart.js

EngineMD soporta dos formas de crear gráficos:

**1. Bloque `chart` (recomendado):**

    ```chart
    {
      "type": "bar",
      "data": {
        "labels": ["Ene", "Feb", "Mar"],
        "datasets": [{"label": "Ventas", "data": [12, 19, 3]}]
      }
    }
    ```

**2. HTML directo:** `<canvas>` + `<script>` en el Markdown.

Ver `MANUAL_CHARTJS.md` para más ejemplos.

## Soporte Obsidian

EngineMD renderiza bóvedas de Obsidian: wikilinks `[[Nota]]`, enlaces a
encabezados `[[Nota#Sección]]`, enlaces a bloques `[[Nota#^id]]`, e
incrustaciones `![[imagen.png|640x480]]` y `![[Nota]]`. La resolución es por
nombre en todo el sitio, como en Obsidian.

Se activa automáticamente, sin configuración: si el sitio tiene una carpeta
`.obsidian/` o si su contenido usa sintaxis Obsidian (`[[...]]` o `![[...]]`).
Ver `MANUAL_OBSIDIAN.md`.

## Temas CSS

| Tema | Descripción |
|------|-------------|
| `github` (default) | Estilo GitHub |
| `dark` | Modo oscuro |
| `simple` | Minimalista |

Se configuran por sitio en `~/.enginemd/settings.json`:

```json
{
  "name": "mi-sitio",
  "css": "dark",
  "js_support": ["mathjax", "mermaid"]
}
```

## Configuración global

`~/.enginemd/settings.json`:

```json
{
  "port": 10300,
  "watch_port": 9696,
  "lang": "en",
  "listing_css": "github",
  "listing_per_page": 20,
  "auto_fetch": true,
  "sri": true,
  "styles": {
    "github": "github.css",
    "dark": "dark.css",
    "simple": "simple.css"
  },
  "directories": [
    { "name": "docs", "path": "/home/user/docs", "active": true,
      "css": "dark", "js_support": ["mathjax", "mermaid"] }
  ]
}
```

## Stack técnico

- **Rust** con axum, comrak, syntect, tera, clap
- Librerías JS/CSS **descargadas y servidas localmente** (offline), con SRI
- CSS themes embehidos en el binario (github, dark, simple)
