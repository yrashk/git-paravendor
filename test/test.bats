setup() {
  load 'test_helper/bats-support/load'
  load 'test_helper/bats-assert/load'
  DIR="$( cd "$( dirname "$BATS_TEST_FILENAME" )" >/dev/null 2>&1 && pwd )"
  TOPDIR=$(realpath "$DIR/..")
  PATH="$DIR/..:$PATH"
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

@test "init with dirty working directory" {
  touch test
  run git paravendor init
  assert_failure
}

@test "add and list dependencies" {
  run git paravendor init
  assert_success
  run git paravendor add "$TOPDIR"
  assert_success
  run git paravendor list
  assert_line "$TOPDIR"
} 

@test "add and clone dependency" {
  run git paravendor init
  assert_success
  run git paravendor add "$TOPDIR"
  assert_success
  ref=$(git paravendor show-ref "$TOPDIR" master)
  run git clone . --no-checkout t && cd t && git checkout "$ref"
  assert_success
}
