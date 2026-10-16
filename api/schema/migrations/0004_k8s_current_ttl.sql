-- 0004: an object no collection lists any more leaves k8s_resources_current.
--
-- A live object is collected again every cycle, so its row keeps a fresh
-- collected_at; a deleted one stopped being sent and kept its last row (pods
-- replaced in a rollout stayed there as Failed). A day is far beyond any
-- collection interval. ClickHouse applies it as parts merge, without
-- rewriting the table now.

ALTER TABLE k8s_resources_current MODIFY TTL toDateTime(collected_at) + INTERVAL 1 DAY SETTINGS materialize_ttl_after_modify = 0;
