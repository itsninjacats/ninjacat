CREATE USER c##datadog IDENTIFIED BY datadog CONTAINER = ALL;
ALTER USER c##datadog SET CONTAINER_DATA=ALL CONTAINER=CURRENT;
GRANT CREATE SESSION TO c##datadog CONTAINER=ALL;
GRANT SELECT ANY DICTIONARY TO c##datadog CONTAINER=ALL;
GRANT SELECT_CATALOG_ROLE TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$session TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$database TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$containers TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sqlstats TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$instance TO c##datadog CONTAINER=ALL;
GRANT SELECT ON dba_feature_usage_statistics TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sql_plan_statistics_all TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$process TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$con_sysmetric TO c##datadog CONTAINER=ALL;
GRANT SELECT ON cdb_tablespace_usage_metrics TO c##datadog CONTAINER=ALL;
GRANT SELECT ON cdb_tablespaces TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sqlcommand TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$datafile TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sysmetric TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sgainfo TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$pdbs TO c##datadog CONTAINER=ALL;
GRANT SELECT ON cdb_services TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$osstat TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$parameter TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sql TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$pgastat TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$asm_diskgroup TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$rsrcmgrmetric TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$dataguard_config TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$dataguard_stats TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$transaction TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$locked_object TO c##datadog CONTAINER=ALL;
GRANT SELECT ON dba_objects TO c##datadog CONTAINER=ALL;
GRANT SELECT ON cdb_data_files TO c##datadog CONTAINER=ALL;
GRANT SELECT ON dba_data_files TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$archive_dest TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$archived_log TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$recovery_file_dest TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$restore_point TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sesstat TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$statname TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$lock TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$sqlarea TO c##datadog CONTAINER=ALL;
GRANT SELECT ON v_$active_session_history TO c##datadog CONTAINER=ALL;
CREATE OR REPLACE VIEW dd_session AS
SELECT /*+ push_pred(sq) push_pred(sq_prev) */
  s.indx as sid,
  s.ksuseser as serial#,
  s.ksuudlna as username,
  DECODE(BITAND(s.ksuseidl, 9), 1, 'ACTIVE', 0, DECODE(BITAND(s.ksuseflg, 4096), 0, 'INACTIVE', 'CACHED'), 'KILLED') as status,
  s.ksuseunm as osuser,
  s.ksusepid as process,
  s.ksusemnm as machine,
  s.ksusemnp as port,
  s.ksusepnm as program,
  DECODE(BITAND(s.ksuseflg, 19), 17, 'BACKGROUND', 1, 'USER', 2, 'RECURSIVE', '?') as type,
  s.ksusesqi as sql_id,
  sq.force_matching_signature as force_matching_signature,
  s.ksusesph as sql_plan_hash_value,
  s.ksusesesta as sql_exec_start,
  s.ksusesql as sql_address,
  CASE WHEN BITAND(s.ksusstmbv, POWER(2, 04)) = POWER(2, 04) THEN 'Y' ELSE 'N' END as in_parse,
  CASE WHEN BITAND(s.ksusstmbv, POWER(2, 07)) = POWER(2, 07) THEN 'Y' ELSE 'N' END as in_hard_parse,
  s.ksusepsi as prev_sql_id,
  s.ksusepha as prev_sql_plan_hash_value,
  s.ksusepesta as prev_sql_exec_start,
  sq_prev.force_matching_signature as prev_force_matching_signature,
  s.ksusepsq as prev_sql_address,
  s.ksuseapp as module,
    s.ksuseact as action,
    s.ksusecli as client_info,
    s.ksuseltm as logon_time,
    s.ksuseclid as client_identifier,
    s.ksusstmbv as op_flags,
    decode(s.ksuseblocker,
        4294967295, 'UNKNOWN', 4294967294, 'UNKNOWN', 4294967293, 'UNKNOWN', 4294967292, 'NO HOLDER', 4294967291, 'NOT IN WAIT',
        'VALID'
    ) as blocking_session_status,
    DECODE(s.ksuseblocker,
        4294967295, TO_NUMBER(NULL), 4294967294, TO_NUMBER(NULL), 4294967293, TO_NUMBER(NULL),
        4294967292, TO_NUMBER(NULL), 4294967291, TO_NUMBER(NULL), BITAND(s.ksuseblocker, 2147418112) / 65536
    ) as blocking_instance,
    DECODE(s.ksuseblocker,
        4294967295, TO_NUMBER(NULL), 4294967294, TO_NUMBER(NULL), 4294967293, TO_NUMBER(NULL),
        4294967292, TO_NUMBER(NULL), 4294967291, TO_NUMBER(NULL), BITAND(s.ksuseblocker, 65535)
    ) as blocking_session,
    DECODE(s.ksusefblocker,
        4294967295, 'UNKNOWN', 4294967294, 'UNKNOWN', 4294967293, 'UNKNOWN', 4294967292, 'NO HOLDER', 4294967291, 'NOT IN WAIT', 'VALID'
    ) as final_blocking_session_status,
    DECODE(s.ksusefblocker,
        4294967295, TO_NUMBER(NULL), 4294967294, TO_NUMBER(NULL), 4294967293, TO_NUMBER(NULL), 4294967292, TO_NUMBER(NULL),
        4294967291, TO_NUMBER(NULL), BITAND(s.ksusefblocker, 2147418112) / 65536
    ) as final_blocking_instance,
    DECODE(s.ksusefblocker,
        4294967295, TO_NUMBER(NULL), 4294967294, TO_NUMBER(NULL), 4294967293, TO_NUMBER(NULL), 4294967292, TO_NUMBER(NULL),
        4294967291, TO_NUMBER(NULL), BITAND(s.ksusefblocker, 65535)
    ) as final_blocking_session,
    DECODE(w.kslwtinwait,
        1, 'WAITING', decode(bitand(w.kslwtflags, 256), 0, 'WAITED UNKNOWN TIME',
        decode(round(w.kslwtstime / 10000), 0, 'WAITED SHORT TIME', 'WAITED KNOWN TIME'))
    ) as STATE,
    e.kslednam as event,
    e.ksledclass as wait_class,
    w.kslwtstime as wait_time_micro,
    c.name as pdb_name,
    sq.sql_text as sql_text,
    sq.sql_fulltext as sql_fulltext,
    sq_prev.sql_fulltext as prev_sql_fulltext,
    comm.command_name
FROM
  x$ksuse s,
  x$kslwt w,
  x$ksled e,
  v$sql sq,
  v$sql sq_prev,
  v$containers c,
  v$sqlcommand comm
WHERE
  BITAND(s.ksspaflg, 1) != 0
  AND BITAND(s.ksuseflg, 1) != 0
  AND s.inst_id = USERENV('Instance')
  AND s.indx = w.kslwtsid
  AND w.kslwtevt = e.indx
  AND s.ksusesqi = sq.sql_id(+)
  AND decode(s.ksusesch, 65535, TO_NUMBER(NULL), s.ksusesch) = sq.child_number(+)
  AND s.ksusepsi = sq_prev.sql_id(+)
  AND decode(s.ksusepch, 65535, TO_NUMBER(NULL), s.ksusepch) = sq_prev.child_number(+)
  AND s.con_id = c.con_id(+)
  AND s.ksuudoct = comm.command_type(+)
;

GRANT SELECT ON dd_session TO c##datadog ;
ALTER SESSION SET CONTAINER = FREEPDB1;
CREATE USER shop IDENTIFIED BY shop QUOTA UNLIMITED ON users;
GRANT CREATE SESSION, CREATE TABLE TO shop;
CREATE TABLE shop.orders (id NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, customer VARCHAR2(32), amount NUMBER(12,2));
INSERT INTO shop.orders (customer, amount) SELECT 'c' || MOD(level, 50), level * 1.5 FROM dual CONNECT BY level <= 1000;
COMMIT;
CREATE INDEX shop.orders_customer ON shop.orders (customer);
EXIT;
