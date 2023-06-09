setup() {
  load 'test_helper/bats-support/load'
  load 'test_helper/bats-assert/load'
  DIR="$( cd "$( dirname "$BATS_TEST_FILENAME" )" >/dev/null 2>&1 && pwd )"
  TOPDIR=$(readlink -f "$DIR/..")
  PATH="$DIR/../target/debug:$PATH"
  pushd $(pwd) 
  tmpdir=$(mktemp -d)
  cd "$tmpdir"
  git init
  git commit -m "init" --allow-empty
}

teardown() {
  popd
}

@test "init" {
  run git paravendor init
  assert_success
  run git rev-parse --verify paravendor
  assert_success
}

@test "add and list dependencies" {
  run git paravendor init
  assert_success
  run git paravendor add dep "file://$TOPDIR"
  assert_success
  run git paravendor list
  assert_line "dep file://$TOPDIR"
} 

@test "add duplicate dependency" {
  run git paravendor init
  assert_success
  run git paravendor add dep "file://$TOPDIR"
  assert_success
  run git paravendor add dep "file://$TOPDIR"
  assert_failure
  assert_line "Error: dep has been already added, aborting"
}

@test "add and clone dependency" {
  run git paravendor init
  assert_success
  run git paravendor add paravendor https://github.com/yrashk/git-paravendor
  assert_success
  ref=$(git paravendor show-ref paravendor master)
  run git clone . --no-checkout t && cd t && git checkout "$ref"
  assert_success
}

@test "cloning repo with para-vendoring dependencies" {
  run git paravendor init
  assert_success
  run git paravendor add dep "file://$TOPDIR"
  assert_success
  run git paravendor list
  assert_line "dep file://$TOPDIR"
  tmpdir1=$(mktemp -d)
  run git clone "$tmpdir" "$tmpdir1"
  assert_success
  cd "$tmpdir1"
  run git paravendor list
  refute_line "set up to track"
  assert_line "dep file://$TOPDIR"
  run git rev-parse --abbrev-ref HEAD
  assert_output "master"
} 

@test "cloning repo with para-vendoring dependencies, in a detached checkout" {
  run git paravendor init
  assert_success
  run git paravendor add dep "file://$TOPDIR"
  assert_success
  run git paravendor list
  assert_line "dep file://$TOPDIR"
  tmpdir1=$(mktemp -d)
  run git clone "$tmpdir" "$tmpdir1" --no-checkout
  assert_success
  cd "$tmpdir1"
  run git checkout --detach master
  assert_success
  run git paravendor list
  refute_line "set up to track"
  assert_line "dep file://$TOPDIR"
  run git rev-parse --abbrev-ref paravendor
  assert_output "paravendor"
} 
