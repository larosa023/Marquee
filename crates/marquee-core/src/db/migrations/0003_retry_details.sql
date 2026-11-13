-- 0.1.3: titles marked "couldn't find details" while a catalog key was missing are retried.
UPDATE items SET data = json_remove(data, '$.enrich_tried') WHERE json_extract(data, '$.enrich_tried') IS NOT NULL;
