CREATE INDEX versions_pending_docs ON versions (id) WHERE docs_status = 'pending';
