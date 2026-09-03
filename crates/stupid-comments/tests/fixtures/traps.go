//go:build linux

// Package config loads configuration. This is a godoc comment attached to a
// declaration, which is how Go documents things, and it must not be judged
// as prose no matter how long it gets.
package config

// Load reads the file at path and returns its parsed contents.
func Load(path string) (string, error) {
	url := "http://example.com # not a comment"
	return url, nil
}
