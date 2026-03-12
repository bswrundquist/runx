package main

import (
	"os"

	"github.com/bswr/runx/internal/tools"
)

func main() {
	os.Exit(tools.RunSHX(os.Args))
}
