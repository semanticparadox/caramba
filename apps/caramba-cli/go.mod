module github.com/semanticparadox/caramba/apps/caramba-cli

go 1.22

require (
	github.com/semanticparadox/caramba/libs/caramba-core v0.0.0
	github.com/spf13/cobra v1.8.1
)

require (
	github.com/inconshreveable/mousetrap v1.1.0 // indirect
	github.com/spf13/pflag v1.0.5 // indirect
	gopkg.in/yaml.v3 v3.0.1 // indirect
)

replace github.com/semanticparadox/caramba/libs/caramba-core => ../../libs/caramba-core
