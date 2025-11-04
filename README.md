# Work in progress.

# gest - Sequence-based gesture daemon for Linux

gest is a gesture daemon for Wayland/Linux that maps sequence-based, repeatable touchpad gestures to configurable shell commands.

## Table of contents
- [Features](#features)
- [Configuration](#configuration)
  - [Configuration Format](#configuration-format)
- [CLI](#cli)
- [Contributing](#contributing)
- [License](#license)

## Features

1. Configurable sequences of steps (touch down/up, moves, edge moves).
2. Per-application gestures by regex matching on window class or title.
3. Repeatable gestures by either tapping or sliding.
4. Runtime active-window tracking via wlroots foreign toplevel interface.

## Configuration

Default config path (if `--config-file` is not specified):
1. If `XDG_CONFIG_HOME` is set: `$XDG_CONFIG_HOME/gest/config.yaml`
2. Otherwise: `$HOME/.config/gest/config.yaml`

### Configuration Format

```yaml
import:
  - browser.yaml
  - playerctl.yaml

options:
  move_threshold: 0.15

  edge:
    threshold: 0.05
    sensitivity: 0.5

gestures:
  - name: Previous workspace
    sequence:
      - fingers: 4
        action: move left
    command: hyprctl dispatch workspace r-1
    
  - name: Brightness up
    sequence:
       - fingers: 1
         action: move up
         edge: right
    repeat_mode: slide
    command: brightnessctl set 10%+

application_gestures:
   firefox.*:
      - name: Previous tab
        sequence:
           - fingers: 3
             action: move left
        repeat_mode: tap
        command: ydotool key 29:1 42:1 15:1 15:0 42:0 29:0
```

- `import`: List of additional configuration files to import.
- `options`: Global options for gesture detection.
  - `move_threshold`: Minimum movement (as a fraction of touchpad size) to register a move action.
  - `edge`: Edge detection settings.
    - `threshold`: Distance from edge to consider as edge move.
    - `sensitivity`: Sensitivity multiplier for edge moves.
- `gestures`: List of global gestures.
  - `name`: Name of the gesture.
  - `sequence`: List of steps defining the gesture.
    - `fingers`: Number of fingers involved in the step.
    - `action`: Action type (`move left/right/up/down`, `touch up/down`).
    - `edge` (optional): Edge specification for edge moves (`top`, `bottom`, `left`, `right`).
    - `distance` (optional): Minimum distance (as a fraction of touchpad size) for this step.
  - `repeat_mode` (optional): How the gesture can be repeated (`tap`, `slide`).
  - `command`: Shell command to execute when the gesture is recognized.
- `application_gestures`: Mapping of application regex patterns to their specific gestures. (see [examples/vim.yaml](examples/vim.yaml))

Example configuration files can be found in the [examples](examples) directory.

## CLI

```bash
Usage: gest [OPTIONS]

Options:
  -v, --verbose...                 Output verbosity level
  -c, --config-file <CONFIG_FILE>  Path to configuration file
  -h, --help                       Print help
```

## Contributing

Contributions are welcome! Please open an issue or submit a pull request with your improvements.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
