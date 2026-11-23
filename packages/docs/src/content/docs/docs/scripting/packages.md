---
title: Script packages
order: 3
---

A script package is a set of scripts that Mira installs from a GitHub repository. A package can also hold shared code that other scripts import.

## Install a package

1. Open **Scripts** and select **Install package**.
2. Enter a repository or repository directory in **GitHub link**.
3. To use a branch other than the default, enter it in **Branch**.
4. Select **Install**. You can also select **Install** beside a package under **Recommended**.

Installed packages appear under **Packages** in **Scripts** and are stored in `~/Mira/packages`.

## Update a package

Mira checks packages for updates when it checks for its own updates. An update replaces the package with its newest version from GitHub. If an update would replace local edits, Mira asks before continuing.

## Create a package

A package has a `package.json` with a name and a `mira` object. Its scripts go under `scripts/`. Set `mira.requires` to the minimum Mira version when needed.

```json
{
  "name": "my-scripts",
  "main": "index.ts",
  "mira": { "requires": "0.2.0" }
}
```

A package with shared code looks like this.

```text
my-scripts/
  package.json
  scripts/
    hunt.ts
  index.ts
```

## Share code from a package

Set `main` in `package.json` to the file that exports the shared code. Any script can then import an installed package by its `name`.

```ts
import { hunt } from "my-scripts";
```
