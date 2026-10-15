CREATE LOGIN datadog WITH PASSWORD = 'Lab-Monitor-9x!';
CREATE USER datadog FOR LOGIN datadog;
GRANT CONNECT ANY DATABASE TO datadog;
GRANT VIEW SERVER STATE TO datadog;
GRANT VIEW ANY DEFINITION TO datadog;
GO
USE msdb;
CREATE USER datadog FOR LOGIN datadog;
GRANT SELECT TO datadog;
GO
CREATE DATABASE shop;
GO
USE shop;
CREATE TABLE orders (id INT IDENTITY PRIMARY KEY, customer VARCHAR(32), amount DECIMAL(12,2), created DATETIME2 DEFAULT SYSUTCDATETIME());
CREATE INDEX orders_customer ON orders (customer);
INSERT INTO orders (customer, amount) SELECT TOP 1000 CONCAT('c', ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) % 50), ROW_NUMBER() OVER (ORDER BY (SELECT NULL)) * 1.5 FROM sys.all_objects;
GO
CREATE PROCEDURE dbo.customer_total @customer VARCHAR(32) AS SELECT SUM(amount) FROM orders WHERE customer = @customer;
GO
