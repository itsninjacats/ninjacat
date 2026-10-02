CREATE USER datadog@'%' IDENTIFIED BY 'datadog';
ALTER USER datadog@'%' WITH MAX_USER_CONNECTIONS 10;
GRANT REPLICATION CLIENT ON *.* TO datadog@'%';
GRANT PROCESS ON *.* TO datadog@'%';
GRANT SELECT ON performance_schema.* TO datadog@'%';
CREATE SCHEMA IF NOT EXISTS datadog;
GRANT EXECUTE ON datadog.* TO datadog@'%';
GRANT CREATE TEMPORARY TABLES ON datadog.* TO datadog@'%';
DELIMITER $$
CREATE PROCEDURE datadog.explain_statement(IN query TEXT)
    SQL SECURITY DEFINER
BEGIN
    SET @explain := CONCAT('EXPLAIN FORMAT=json ', query);
    PREPARE stmt FROM @explain;
    EXECUTE stmt;
    DEALLOCATE PREPARE stmt;
END $$
CREATE PROCEDURE datadog.enable_events_statements_consumers()
    SQL SECURITY DEFINER
BEGIN
    UPDATE performance_schema.setup_consumers SET enabled='YES' WHERE name LIKE 'events_statements_%';
    UPDATE performance_schema.setup_consumers SET enabled='YES' WHERE name = 'events_waits_current';
END $$
DELIMITER ;
GRANT EXECUTE ON PROCEDURE datadog.enable_events_statements_consumers TO datadog@'%';
CREATE DATABASE shop;
USE shop;
DELIMITER $$
CREATE PROCEDURE shop.explain_statement(IN query TEXT)
    SQL SECURITY DEFINER
BEGIN
    SET @explain := CONCAT('EXPLAIN FORMAT=json ', query);
    PREPARE stmt FROM @explain;
    EXECUTE stmt;
    DEALLOCATE PREPARE stmt;
END $$
DELIMITER ;
GRANT EXECUTE ON PROCEDURE shop.explain_statement TO datadog@'%';
GRANT SELECT ON shop.* TO datadog@'%';
CREATE TABLE orders (id INT AUTO_INCREMENT PRIMARY KEY, customer VARCHAR(32), amount DECIMAL(12,2), created TIMESTAMP DEFAULT CURRENT_TIMESTAMP, INDEX orders_customer (customer));
INSERT INTO orders (customer, amount) WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM seq WHERE i < 1000) SELECT CONCAT('c', i % 50), i * 1.5 FROM seq;
