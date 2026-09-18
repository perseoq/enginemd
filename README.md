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
| `enginemd fetch` | Descarga librerías JS/CSS desde CDN |
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
enginemd --js-support mathjax,mermaid,chartjs  # Librerías JS
enginemd --css-support dark         # Tema CSS
enginemd --lang es                  # Idioma
enginemd --obsidian                 # Modo Obsidian (wikilinks/embeds)
```

## Modo daemon

Ejecuta el servidor en segundo plano y lo deja corriendo tras reiniciar la
máquina, sin configurar un servicio:

```bash
enginemd daemon start     # arranca en segundo plano + autoarranque @reboot
enginemd daemon status    # estado y autoarranque
enginemd daemon stop      # detiene y desactiva el autoarranque
enginemd daemon restart
```

Los flags globales (`--port`, `--path`, `--lang`, `--obsidian`, ...) se reenvían
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

Se activa automáticamente si el sitio contiene una carpeta `.obsidian/`, o con
`--obsidian`. Ver `MANUAL_OBSIDIAN.md`.

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
  "dependencies": {
    "mathjax": "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js",
    "mermaid": "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.min.js",
    "chartjs": "https://cdn.jsdelivr.net/npm/chart.js@4/dist/chart.umd.min.js",
    "highlight": "https://cdn.jsdelivr.net/npm/@highlightjs/cdn-assets@11/highlight.min.js",
    "katex": "https://cdn.jsdelivr.net/npm/katex@0.16/dist/katex.min.js",
    "fontawesome": "https://cdn.jsdelivr.net/npm/@fortawesome/fontawesome-free@6/css/all.min.css",
    "anchor": "https://cdn.jsdelivr.net/npm/anchor-js@5/anchor.min.js"
  },
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
- Librerías JS cargadas desde CDN (jsdelivr)
- CSS themes embehidos en el binario (github, dark, simple)
