bats_ref=$(shell cargo run --quiet -- show-ref bats-core v1.9.0)
bats_support_ref=$(shell cargo run --quiet -- show-ref bats-support v0.3.0)
bats_assert_ref=$(shell cargo run --quiet -- show-ref bats-assert v2.1.0)

.PHONY: test

test: test/bats test/test_helper/bats-support test/test_helper/bats-assert
	cargo test
	cargo build
	./test/bats/bin/bats test

test/bats: Makefile
	rm -rf test/bats
	git clone . --no-checkout test/bats
	cd test/bats && git checkout ${bats_ref}

test/test_helper/bats-support: Makefile
	rm -rf test/test_helper/bats-support
	git clone . --no-checkout test/test_helper/bats-support
	cd test/test_helper/bats-support && git checkout ${bats_support_ref}

test/test_helper/bats-assert: Makefile
	rm -rf test/test_helper/bats-assert
	git clone . --no-checkout test/test_helper/bats-assert
	cd test/test_helper/bats-assert && git checkout ${bats_assert_ref}

clean:
	rm -rf test/bats test/test_helper/bats-assert test/test_helper/bats-support
