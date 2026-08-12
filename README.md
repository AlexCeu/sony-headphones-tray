# Sony XM4 Tray

Control del modo de sonido de los auriculares Sony WH-1000XM4 desde la bandeja del sistema (KDE Plasma) usando el protocolo **StatusNotifierItem**.

## Características

- Icono de bandeja que muestra el perfil actual aplicado (tooltip) y permite cambiarlo por menú.
- Perfiles: Cancelación de ruido, Reducción de viento, Sonido ambiente, Sin cancelación (OFF).
- Detección automática del dispositivo desde `bluetoothctl devices`; si hay varios, selector gráfico en el menú.
- El perfil elegido se persiste en `~/.config/sony-xm4-tray/` y se reenvía al arrancar si se pide.
- Notificaciones del escritorio (`notify-send`, con fallback a `kdialog`) al cambiar de perfil y al consultar la MAC.

## Requisitos / escritorios

- Bandeja: protocolo **StatusNotifierItem** (KDE, XFCE, LXQt, Cinnamon, MATE, Budgie…).
  En **GNOME** hay que instalar `gnome-shell-extension-appindicator` (SNI no está por defecto).
- **bluez-utils** (`bluetoothctl` para detectar el dispositivo y `sdptool` para hallar el canal RFCOMM).

## Compatibilidad

Mismo protocolo RFCOMM (canal 9, UUID `96cc203e-5068-46ad-b32d-e316f5e069ba`), soportado en:

- WH-1000XM2, WH-1000XM3, WH-1000XM4
- WF-1000XM3
- WH-XB900N
- WI-1000X

## Uso

```sh
sony-xm4-tray
```

- Si no hay MAC configurada, usa el submenú "Elegir dispositivo (Bluetooth)" para seleccionarla.
- Config alternativa: `~/.config/sony-xm4-tray/mac`, variable `XM4_MAC` o `--mac <addr>`.

## Compilar

```sh
cargo build --release
cp target/release/sony-xm4-tray ~/.local/bin/
```

## Autor y créditos

Autor: TheOneProgrammer · https://theoneprogrammer.com

- Código Rust: port del CLI Python
  [sony-headphones-control-py](https://github.com/impankratov/sony-headphones-control-py).
- Protocolo RFCOMM (canal 9, UUID `96cc203e-5068-46ad-b32d-e316f5e069ba`):
  [sony-headphones-control](https://github.com/ClusterM/sony-headphones-control) (ClusterM).

## Licencia

El proyecto está licenciado bajo **GPL-2.0-or-later**. Ver [LICENSE](LICENSE).

> Este proyecto está escrito en Rust y desarrollado/revisado con
> asistencia de IA (opencode), revisado manualmente por su autor.