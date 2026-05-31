# **Aevia Language Specification**

## **1\. Core Philosophy**

Aevia is a high-performance, scientific computing domain-specific language (DSL). It is engineered for dynamic, Just-In-Time (JIT) execution through the RSSN backend while guaranteeing static-level execution safety via algebraic type-checking and dimensional analysis.  
Aevia's design blends the strictness, safety, and module semantics of Rust with the mathematical brevity of standard scientific notation.

## **2\. Type System and Dimensional Analysis**

Units and physical dimensions are first-class language constructs. The Aevia compiler evaluates dimensional equations at compile time using an algebraic solver to guarantee dimensional consistency before runtime.

### **2.1 Base Dimensions and Unit Aliases**

The compiler recognizes seven standard SI base dimensions: m (length), s (time), kg (mass), A (electric current), K (temperature), mol (amount of substance), and cd (luminous intensity).  
// Custom unit and dimensional alias declarations  
type Acceleration \= m / s^2;  
type Newton \= kg \* Acceleration;  
type Joule \= Newton \* m;  
type Pascal \= Newton / m^2;

### **2.2 Variable Declarations and Type Inference**

Variables support complete type inference. However, when physical dimensions are provided, the dimension solver strictly enforces them.  
let mass: kg \= 10.0;  
let acc: m/s^2 \= 9.8;

// The compiler infers 'force' to be of type \`kg \* m / s^2\`, which matches 'Newton'  
let force: Newton \= mass \* acc; 

### **2.3 Structural Physical Types**

Structs can encapsulate physical properties, requiring explicit dimension fields.  
pub struct Particle {  
    pub position: m,  
    pub velocity: m / s,  
    pub mass: kg,  
}

## **3\. Module System, Visibility, and Imports**

Aevia implements a structured modular system to manage namespaces, visibility, and external integrations.

### **3.1 Visibility Modifiers**

By default, all module declarations, structs, functions, and constants are **private** to their defining module. Aevia supports three tiers of visibility:

* pub: Publicly accessible from any importing crate or module.  
* pub(crate): Visible only within the parent crate boundaries.  
* *Private (No prefix)*: Visible only within the current module scope and its child modules.

### **3.2 Hierarchical Module Structure**

Modules are declared using the mod keyword and map directly to the directory hierarchy.  
// Declares a submodule named 'kinematics' (looks for kinematics.aevia or kinematics/mod.aevia)  
pub mod kinematics; 

pub mod mechanics {  
    // Nested private submodule  
    mod internal\_solver;  
      
    pub(crate) struct State {  
        pub t: s,  
    }  
}

### **3.3 The use Statement and Aliasing**

Namespaces are imported and scoped using the use keyword. It supports nested paths, glob imports, and custom renaming via as.  
use mechanics::kinematics::{Force as F, Mass as M};  
use mechanics::State;  
use math::constants::\*;

## **4\. Advanced Operator Definition (The op DSL)**

The op keyword allows developers to declare custom, high-level mathematical operators. These declarations compile directly into CustomOpDescriptor structures within the RSSN runtime backend.  
pub op custom(x: m \-\> m/s) {  
    // Pipeline optimization constraints  
    properties {  
        vectorizable: true,  
        commutative: true,  
        associative: true,  
        cost: 1.5,  
    }

    // Algebraic simplifying compiler-passes (applied to the AST)  
    simplify {  
        custom(0) \=\> 0,  
        custom(custom(y)) \=\> y,  
    }  
      
    // E-graph rules for equivalence saturation and hardware scheduling  
    egraph {  
        rewrite custom(y) \=\> integrate(y, dy)  
    }  
}

## **5\. Function Syntax and Syntactic Sugar**

Aevia distinguishes between **expression-bodied** functions and **block-bodied** functions. This architectural boundary determines how the JIT compiler handles loop/kernel fusion and JIT dispatching.

### **5.1 Expression-bodied Functions (:=)**

Expression-bodied functions are optimized for inline execution, automatic loop fusion, and mathematical rewriting. They cannot contain local imperative scopes.  
// Auto-inlined and parsed by the E-graph optimization pass  
pub fn force(m: kg, a: Acceleration) \-\> Newton := m \* a;

### **5.2 Block-bodied Functions ({})**

Block-bodied functions allow complex local variable mutation, loop execution, and intermediate structures. They implicitly run within a **phaser context**—a lightweight execution fiber context using dtact that coordinates iterative convergence loops.  
// Block-bodied function: Enters the local simulation context  
pub fn calculate\_potential(x: m, k: Newton/m) \-\> Joule {  
    let scalar \= 0.5;  
    scalar \* k \* x^2  
}

## **6\. Control Flow and Dynamic Execution**

Control flow operations in Aevia are mapped to structured control nodes in the JIT compilation Directed Acyclic Graph (DAG) for optimized pipeline generation.

### **6.1 Control Flow Structures**

Aevia supports standard imperative keywords: for, while, loop, if, elseif, else, and match.  
pub fn simulate\_decay(mut half\_life: s, decay\_constant: 1/s) {  
    loop {  
        if half\_life \< 1.0\_s {  
            break;  
        } elseif half\_life \> 100.0\_s {  
            half\_life \= half\_life \* 0.5;  
        } else {  
            half\_life \= half\_life \- (decay\_constant \* half\_life \* 1.0\_s);  
        }  
    }  
}

### **6.2 Match Patterns and Dimensional Safety**

Pattern matching acts as a compiler-guaranteed guard for handling optional JIT values and executing dimension-safe code branches.  
match op\_result {  
    Some(v: kg) \=\> print(v),  
    Some(v: m) \=\> log("Length detected, expected mass"),  
    None \=\> log("invalid dimension or empty node"),  
}

### **6.3 Lower-level System Escapes (transmute)**

For direct physical-to-virtual buffer manipulation, Aevia provides an unsafe transmute keyword designed to map untyped flat memory arrays to typed physical tensors.  
// Cast raw memory directly to a spatial physical tensor  
let raw\_data: tensor\[4,4\] \= unsafe transmute(buffer) {  
    assume\_unit: kg/m^3,  
    assume\_layout: row\_major,  
};

## **7\. The Macro System**

Aevia incorporates meta-programming to abstract redundant mathematical setups and direct compiler-level optimization passes.

### **7.1 Declarative Macros**

Using a macro\_rules\! format, developers can construct code generation rules for repetitive scientific structures.  
macro\_rules\! create\_vector\_type {  
    ($name:ident, $unit:ty) \=\> {  
        struct $name {  
            x: $unit,  
            y: $unit,  
            z: $unit,  
        }  
    };  
}

create\_vector\_type\!(Position3D, m);

### **7.2 Procedural Macros (Compiler Attribute Hooks)**

Procedural attributes run inline within the Aevia JIT pipeline to instruct compiler backends on optimization heuristics.

* \#\[simplify\_fusion\]: Forces aggressive loop fusion and mathematical reduction.  
* \#\[jit\_kernel\]: Directs the RSSN engine to pre-compile this functional branch directly into target machine code (AVX-512, CUDA, or Metal kernels).

\#\[jit\_kernel\]  
\#\[simplify\_fusion\]  
pub fn process\_tensor(inputs: tensor\[1024, 1024\]) \-\> tensor\[1024, 1024\] {  
    // Compilation node optimization block  
}

## **8\. Document Comments**

All modules, APIs, and structs must be documented with semantic metadata.

* //\!: Declares module-level documentation. Must be placed at the top of the file.  
* ///: Document comments for public types, structs, and functions.  
* /// \# Physical Context: A mandatory section in function docs indicating numerical boundaries, domain limits, and expected outputs.

//\! \# Dynamics Module  
//\!  
//\! Provides mechanical simulation tools and kinetic energy solvers.

/// Calculates kinetic energy.  
///  
/// \# Physical Context  
/// \- domain: mass \> 0.0, velocity \>= 0.0  
/// \- unit\_output: Joule  
pub fn kinetic\_energy(m: kg, v: m/s) \-\> Joule := 0.5 \* m \* v^2;

## **9\. Project Toolchain and Standards**

Aevia features a unified compiler driver and build system (aevia) configured through an Aevia.toml manifest file.

### **9.1 The Aevia.toml File**

\[package\]  
name \= "molecular\_dynamics"  
version \= "2.1.0"  
authors \= \["Scientific Computing Group"\]

\[profile.release\]  
opt-level \= 3  
backend \= "rssn-gpu"  
dimension-checking \= "strict"

\[dependencies\]  
linear\_algebra \= { version \= "1.4", registry \= "aevia-central" }

### **9.2 Project Directory Architecture**

my\_simulation/  
├── Aevia.toml              \# Configuration and Dependency settings  
├── src/  
│   ├── main.aevia          \# Application Entry-Point  
│   ├── lib.aevia           \# Exported Library Functions  
│   └── modules/            \# Structural Submodules  
│       └── physics.aevia  
├── tests/                  \# Integration and Dimensional Validation Tests  
└── kernels/                \# Precompiled custom SIMD or assembly routines

### **9.3 Toolchain Operations**

* aevia fmt: Automatically formats syntax structure.  
* aevia new: Automatically creates a new aevia project.  
* aevia lint: Performs static analysis, catching illegal dynamic unit conversions and bad memory mappings.  
* aevia doc: Compiles system comments and physical schemas into clean Markdown/HTML documentation.  
* aevia build: Compiles physical representations down to the optimized RSSN computation graph.
* aevia shell: Opens the interactive shell of aevia. Aevia shall support running single file code and using interactive shells anyway. 

## **10\. JIT Compiler Pipeline**

       \+-------------------------+  
       |   Source Code (.aevia)   |  
       \+------------+------------+  
                    |  
                    v  
       \+------------+------------+  
       |     Frontend Parser     | \-\> Ast Graph & Metadata  
       \+------------+------------+  
                    |  
                    v  
       \+------------+------------+  
       |    Dimension Solver     | \-\> Checks dimensional consistency  
       \+------------+------------+  
                    |  
                    v  
       \+------------+------------+  
       |  E-Graph Saturation Engine  | \-\> Applies Simplification/Rewrites  
       \+------------+------------+  
                    |  
                    v  
       \+------------+------------+  
       | Kernel Fusion Partition | \-\> Groups operations into Kernels  
       \+------------+------------+  
                    |  
                    v  
       \+------------+------------+  
       | RSSN Backend Dispatcher  | \-\> Generates Target CPU/GPU Machine Code  
       \+-------------------------+  



### Recommended Project Structure

```
aevia-compiler/
├── Cargo.toml
├── src/
│   ├── main.rs                 # CLI entrypoint
│   ├── lib.rs
│   ├── parser/                 # .aevia grammar (chumsky)
│   ├── ast.rs                  # Aevia AST definitions
│   ├── types/                  # Dimension solver + type checker
│   ├── lowering/               # Aevia → RSSN DAG + CustomOp
│   ├── modules/                # Module system & imports
│   ├── ops/                    # 'op' keyword handling
│   ├── diagnostics.rs          # Error reporting
│   └── commands/               # build, fmt, new, lint, shell...
├── examples/
├── tests/
└── Aevia.toml.example
```

---

**Updated with `.ae` extension**

Here's the revised recommendation using the cleaner `.ae` file extension.

---


### Updated Project Directory Structure

```
my_simulation/
├── Aevia.toml                  # Project configuration
├── src/
│   ├── main.ae                 # Main entry point
│   ├── lib.ae                  # Library root (if it's a library)
│   └── modules/
│       ├── physics.ae
│       ├── kinematics.ae
│       └── particle.ae
├── tests/                      # Test files
│   └── integration.ae
├── kernels/                    # Precompiled kernels
└── docs/                       # Generated documentation
```

---

### Updated References in the Language

You should update the Aevia Language Specification accordingly:

- File extension: **`.ae`**
- Example file names: `main.ae`, `solver.ae`, `particle_system.ae`
- In documentation and error messages: refer to "Aevia source files (`*.ae`)"

**Toolchain commands examples:**
```bash
aevia build main.ae
aevia run simulation.ae
aevia fmt src/**/*.ae
```

---