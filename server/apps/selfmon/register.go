package selfmon

import (
	"fmt"

	"ergo.services/ergo/gen"
)

// RegisterTypes makes this package's message types serializable across nodes.
//
// Collect is the only one: the metrics themselves leave as storage.WriteMetrics,
// which apps/storage registers. Once roles are split, a node without storage
// will still want to sample itself and send the points to the node that has
// the writers — at which point this registration is what makes it work.
func RegisterTypes(node gen.Node) error {
	if err := node.Network().RegisterTypes([]any{Collect{}}); err != nil {
		return fmt.Errorf("selfmon: register types: %w", err)
	}
	return nil
}
