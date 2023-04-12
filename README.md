# git paravendor

An external git subcommand that allows one to store git dependencies inside of
their git repositories.

The idea here is that instead of vendoring snapshots (lost history) or relying
on external repositories, one can carry all these independent git histories
inside of their projects.

This helps with workflows in absence of a good internet connection, and ensures
that dependencies that are gone are not going to have an immediate disrupting
impact on your project.

# Workflow

## Initialize

```shell
git paravendor init
```

### Vendoring

```shell
git paravendor add <name> <git repo url>
```

### Syncing dependencies

```shell
git paravendor sync [<name>]...
```

If URL is not provided, it will sync all repostories.

## Listing dependencies

```shell
git paravendor list
```

## Checking out dependencies

```shell
ref=$(git paravendor show-ref <name> <ref/branch/tag name>)
git clone . --no-checkout <dependency> && cd <dependecy>
git checkout $ref
```