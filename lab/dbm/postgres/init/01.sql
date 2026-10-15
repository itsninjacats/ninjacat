CREATE USER datadog WITH PASSWORD 'datadog';
GRANT pg_monitor TO datadog;
CREATE DATABASE shop;
\c shop
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
CREATE SCHEMA datadog;
GRANT USAGE ON SCHEMA datadog TO datadog;
GRANT USAGE ON SCHEMA public TO datadog;
CREATE OR REPLACE FUNCTION datadog.explain_statement(l_query TEXT, OUT explain JSON) RETURNS SETOF JSON AS
$$
DECLARE curs REFCURSOR; plan JSON;
BEGIN
  OPEN curs FOR EXECUTE pg_catalog.concat('EXPLAIN (FORMAT JSON) ', l_query);
  FETCH curs INTO plan; CLOSE curs; RETURN QUERY SELECT plan;
END;
$$ LANGUAGE 'plpgsql' RETURNS NULL ON NULL INPUT SECURITY DEFINER;
CREATE TABLE orders (id serial PRIMARY KEY, customer text, amount numeric, created timestamptz DEFAULT now());
INSERT INTO orders (customer, amount) SELECT 'c' || (i % 50), i * 1.5 FROM generate_series(1, 5000) i;
CREATE INDEX orders_customer ON orders (customer);
ANALYZE orders;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO datadog;
