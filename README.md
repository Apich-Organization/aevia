# Aevia: High-Performance Scientific DSL

**Aevia** is a modern, high-performance scientific computing Domain-Specific Language (DSL) built on the **RSSN** backend. It combines the expressiveness of mathematical notation with the safety and performance of a compiled language.

![Aevia Logo](path/to/aevia-logo.png)

## Features

- **🚀 Blazing-fast execution**: Powered by the **RSSN** runtime with Just-In-Time (JIT) compilation
- **⚛️ Algebraic dimensional analysis**: Compile-time enforcement of physical units and dimensions
- **🦀 Rust-inspired safety**: Module system, strict type checking, and memory safety
- **✨ Elegant syntax**: Clean, mathematically-focused syntax with optional type inference
- **🔗 Rich ecosystem**: Seamless integration with Rust libraries via FFI

## Getting Started

### Installation

```bash
# Clone the repository
git clone https://github.com/Apich-Organization/aevia.git
cd aevia

# Build the toolchain
cargo build --release
```

### Quick Start

Create your first Aevia project:

```bash
# Create a new project
aevia new my_simulation
cd my_simulation

# Build and run
aevia build src/main.ae
aevia run src/main.ae
```

## Language Features

### Dimensional Analysis

Units are first-class citizens in Aevia. The compiler validates dimensions at compile time to prevent physical impossibilities.

```ae
// Define custom units
type Acceleration = m / s^2;
type Force = kg * Acceleration;

// Dimensional analysis in action
let mass: kg = 10.0;
let acc: Acceleration = 9.8;

// This will compile successfully
let force: Force = mass * acc;

// This will cause a compile-time error
// let invalid: m = force; 
```

### Module System

Aevia uses a module system inspired by Rust for organizing code and managing visibility.

```ae
// physics.ae
pub mod kinematics {  
    pub(crate) struct State {  
        pub position: m,
    }
}

// main.ae
use physics::kinematics::{Force as F, Mass as M};
```

### Custom Operators

Define high-level mathematical operators with optimization metadata.

```ae
pub op custom(x: m -> m/s) {
    properties {
        vectorizable: true,
        commutative: true,
    }
    
    simplify {
        custom(0) => 0,
    }
}
```

## Project Structure

```
my_simulation/
├── Aevia.toml                    # Project configuration
├── src/
│   ├── main.ae                   # Main entry point
│   ├── lib.ae                    # Library root
│   └── modules/
│       ├── physics.ae
│       └── kinematics.ae
├── tests/                        # Test files
└── kernels/                      # Precompiled kernels
```

## Tooling

Aevia comes with a powerful CLI tool for managing your projects.

| Command | Description |
|---------|-------------|
| `aevia new <name>` | Create a new project |
| `aevia build <file>` | Build an Aevia file |
| `aevia run <file>` | Build and run |
| `aevia check <file>` | Type-check without compiling |
| `aevia fmt <path>` | Auto-format code |
| `aevia test` | Run tests |

## Contributing

Contributions are welcome! Please read our [CONTRIBUTING.md](path/to/CONTRIBUTING.md) for guidelines.

## License

This project is dual-licensed under the **MIT License** and the **Apache 2.0 License**. See [LICENSE](path/to/LICENSE) for details.

## Community

Join the conversation:

- **Discord**: [Join our Discord server](https://discord.gg/yourserver)
- **GitHub**: Follow us on [GitHub](https://github.com/Apich-Organization/aevia)
- **Website**: [Learn more at our website](https://aevia-lang.org)

## Contact

For questions or support, please reach out to our team at [EMAIL_ADDRESS]

---

**Built with ❤️ by the Aevia Development Team at Apich Organization**

![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)
![Apache 2.0 License](https://img.shields.io/badge/license-Apache%202.0-orange.svg)
![GitHub Stars](https://img.shields.io/github/stars/Apich-Organization/aevia)
![Rust Version](https://img.shields.io/badge/rust-%3E%3D1.75-green.svg)
![Made with Aevia](https://img.shields.io/badge/Made%20with-Aevia-blueviolet)
