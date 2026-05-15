# x-env

Run programs with environment variables from config files.

## Features

- Parse environment variables from `.env` files
- Support `[env]`, `[exe]`, `[p-args]` sections
- Cross-platform (Windows/Linux)
- Windows right-click menu integration
- Path conversion for msys compatibility

## Config File Format

```ini
[env]
KEY1=value1
KEY2=value2

[exe]
program_name.exe

[p-args]
--argument1
value1
```

## Usage

```bash
# Run with default .env file
x-env python app.py

# Specify env file
x-env -f my.env node app.js

# Set working directory
x-env --cwd /path/to/dir node app.js

# Use [exe] from config (no program argument needed)
x-env -- --port 3000
```

## Windows Right-Click Menu

1. Place `x-env.exe` at `D:\Path\x-env.exe`
2. Double-click `x-env-menu-install.reg` to install
3. Right-click on any directory → "x-env"

## Build

```bash
cargo build --release
```
