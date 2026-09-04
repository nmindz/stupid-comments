# Entry points for the build.
.PHONY: build test

build:
	@echo "compiling with -DFOO=1 # not a comment"
	go build -o bin/app ./cmd/app

test:
	@echo '# also not a comment'
	go test ./...
