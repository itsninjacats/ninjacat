CREATE DATABASE shop;
\c shop
CREATE TABLE users (id serial PRIMARY KEY, email text, name varchar(64), note text);
INSERT INTO users (email, name, note) VALUES
  ('alice@example.com', 'Alice', 'card 4111111111111111 on file'),
  ('bob@example.com', 'Bob', NULL),
  ('no email here', 'Carol', 'writes to carol@example.org and dave@example.org');
CREATE TABLE orders (id serial PRIMARY KEY, amount numeric);
INSERT INTO orders (amount) SELECT i FROM generate_series(1, 20) i;
