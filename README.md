# EngineMD

Servidor de Markdown a HTML, al estilo GitHub Pages. Convierte archivos `.md`
en páginas web con temas, resaltado de código, matemáticas, diagramas, gráficos
y soporte de bóvedas de Obsidian.

📖 **Documentación completa: [MANUAL.md](MANUAL.md)**

## Características

- Markdown (comrak) con tablas, notas al pie, tasklists y autolinks.
- **Tema automático**: claro = VS Code Light+, oscuro = Dracula; detecta el tema del PC y tiene conmutador.
- Resaltado de código en servidor (syntect).
- **Assets self-hosted**: MathJax, KaTeX, Mermaid, Chart.js, highlight, anchor y
  Font Awesome se descargan y se sirven localmente (funciona offline).
- **Soporte Obsidian** automático: wikilinks, embeds, transclusión, block refs y
  callouts.
- **Hot-reload** (`--watch`) y **modo daemon** con autoarranque.
- **Export estático** (`build`) para GitHub Pages.
- TOC plegable, botón de copiar, compresión gzip y healthcheck.

## Instalación

```bash
git clone <repo>
cd enginemd
cargo build --release
sudo cp target/release/enginemd /usr/local/bin/   # instalar en el PATH
enginemd fetch                                    # descarga las librerías JS/CSS
```

## Uso rápido

```bash
# Crear un sitio nuevo
enginemd new mi-docs

# Servidor (http://0.0.0.0:10300)
enginemd

# Servidor de desarrollo con hot-reload (http://0.0.0.0:9696)
enginemd --watch
```

## Comandos

| Comando | Descripción |
|---------|-------------|
| `enginemd new <name>` | Crea un sitio nuevo en `~/.enginemd/sites/<name>/` |
| `enginemd up <path>` | Registra un directorio existente como sitio |
| `enginemd down <name>` | Elimina un sitio del listado |
| `enginemd fetch [--force]` | Descarga las librerías JS/CSS al caché local |
| `enginemd build --out DIR` | Exporta los sitios a HTML estático |
| `enginemd` | Inicia el servidor en `0.0.0.0:10300` |
| `enginemd --watch` | Servidor dev con hot-reload (`0.0.0.0:9696`) |
| `enginemd daemon start\|stop\|restart\|status\|logs` | Gestiona el servidor en segundo plano |

## Flags del servidor

```bash
enginemd --path /ruta               # sirve un solo directorio
enginemd --port 8080                # puerto personalizado
enginemd --lang es                  # idioma
enginemd --js-support mathjax,mermaid,chartjs  # allowlist de librerías
```

## Documentación

- **[MANUAL.md](MANUAL.md)** — manual completo: comandos, assets, Chart.js,
  Obsidian, daemon, export estático, configuración y temas.
- `Dockerfile` — imagen de contenedor.
- `.github/workflows/ci.yml` — integración continua.

## Stack técnico

- **Rust** con axum, comrak, syntect, tera y clap.
- Librerías JS/CSS **descargadas y servidas localmente** (offline), con SRI.
- CSS themes embebidos en el binario.
