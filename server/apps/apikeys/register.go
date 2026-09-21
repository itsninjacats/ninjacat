package apikeys

import (
	"fmt"

	"ergo.services/ergo/gen"
)

// RegisterTypes makes this package's message types serializable across nodes.
// See apps/storage/register.go for the full reasoning.
func RegisterTypes(node gen.Node) error {
	types := []any{
		Refresh{},
		Key{},
	}
	if err := node.Network().RegisterTypes(types); err != nil {
		return fmt.Errorf("apikeys: register types: %w", err)
	}
	return nil
}
