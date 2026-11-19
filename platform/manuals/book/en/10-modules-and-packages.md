# Modules and packages

A project grows from a file into a group of named modules. Use a module to keep an implementation goal together, and a package to state how a collection is built and shared. A module's public exports are the surface other parts of the project depend on.

## Start with two files

In an empty directory, create `Geometry/Shapes.aev` and `main.aev`. Put the following in `Geometry/Shapes.aev`.

```aevia,file=Geometry/Shapes.aev
module Geometry.Shapes

fn rectangle(width: Real[Length], height: Real[Length]): Real[Length^2] =
    width * height
```

Then put the following in `main.aev`.

```aevia,file=main.aev
import Geometry.Shapes as Shapes

fn main(): IO Unit =
    println(Shapes.rectangle(3 m, 4 m))
```

Run `aevia run main.aev` from this directory. In a one-off program the import can resolve from the entry's directory. Keep the name and case of each path component consistent. In a package the import tree comes from its source directory and dependencies.

## Create a package

Run `aevia init specimen`. The resulting `aevia.toml` names the package and its entry file. Use `aevia check --all` to inspect the package, `aevia test` to run checks, and `aevia doc --out docs` to render the `##` comments on its public surfaces. Place multi-file examples beside the code as independent entry points and label the input each one expects.

Use a local path dependency while two packages evolve together. Keep `aevia.lock` in an application when a collaborator must reproduce the resolved versions. Fetch locked registry contents before building offline. Run `aevia --locked check --all` where an unexpected change to dependency resolution should fail.

## Prepare to share

Describe each public parameter in its physical terms, explain failure conditions, and include a small example with an expected result. `aevia publish --dry-run` prepares and validates a package without uploading it. Registry publishing uses a registered signing key. The [registry documentation](/docs?lang=en) shows the published versions and their generated API pages.
