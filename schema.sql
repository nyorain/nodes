PRAGMA encoding = "UTF-8";

DROP TABLE IF EXISTS nodes;
DROP TABLE IF EXISTS tags;

CREATE TABLE nodes (
	id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
	content TEXT NOT NULL,
	-- mimetype TEXT NOT NULL, -- strictly follow mime standard
	created DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, -- creation date
	edited DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, -- last edit date (edit command invoked)
	viewed DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, -- last date viewed (edit/show command invoked)
	archived BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE tags (
	node INTEGER NOT NULL,
	tag text NOT NULL,
	PRIMARY KEY(node, tag),
	CONSTRAINT fk_node
		FOREIGN KEY (node)
		REFERENCES nodes(id)
);

-- idea: links
/*
CREATE TABLE links (
	fromnode INTEGER NOT NULL PRIMARY KEY,
	tonode INTEGER NOT NULL PRIMARY KEY,
	CONSTRAINT fk_fromnode
		FOREIGN KEY (fromnode)
		REFERENCES nodes(id)
	CONSTRAINT fk_tonode
		FOREIGN KEY (tonode)
		REFERENCES nodes(id)
);
*/

-- idea: storage types:
-- Allow to just store filepath in nodes.content, add column storagetype
-- 0: whole node in 'content' field
-- 1: 'content' is utf8 encoded relative filepath
-- textual data always expected in utf8 encoding

