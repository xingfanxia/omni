"""Unit tests for DocumentsRepository permission filtering.

These test the SQL permission filter via get_by_id(user_email=...) against
a real ParadeDB instance, so they live in unit/ but use the db_pool fixture.
"""

import json

import pytest
from ulid import ULID

from db import UsersRepository
from db.documents import DocumentsRepository

pytestmark = pytest.mark.integration


async def _ensure_user(db_pool) -> str:
    users_repo = UsersRepository(pool=db_pool)
    user = await users_repo.create(
        email=f"{ULID()}@test.local",
        password_hash="not-a-real-hash",
        full_name="Test User",
    )
    return user.id


async def _insert_doc(db_pool, created_by: str, permissions: dict) -> str:
    doc_id = str(ULID())
    source_id = str(ULID())
    async with db_pool.acquire() as conn:
        await conn.execute(
            """INSERT INTO sources (id, source_type, name, is_active, is_deleted, created_by)
               VALUES ($1, 'google_drive', 'Test', true, false, $2)""",
            source_id,
            created_by,
        )
        await conn.execute(
            """INSERT INTO documents (id, source_id, external_id, title, content_type, permissions)
               VALUES ($1, $2, $3, $4, 'text/plain', $5::jsonb)""",
            doc_id,
            source_id,
            f"ext_{doc_id}",
            "Test Doc",
            json.dumps(permissions),
        )
    return doc_id


@pytest.fixture
async def user_id(db_pool) -> str:
    return await _ensure_user(db_pool)


@pytest.fixture
def repo(db_pool) -> DocumentsRepository:
    return DocumentsRepository(db_pool)


class TestGetByIdPermissionFilter:
    @pytest.mark.asyncio
    async def test_no_email_returns_doc_without_filtering(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": False, "users": ["alice@co.com"], "groups": []}
        )
        doc = await repo.get_by_id(doc_id)
        assert doc is not None
        assert doc.id == doc_id

    @pytest.mark.asyncio
    async def test_public_doc_accessible_to_anyone(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": True, "users": [], "groups": []}
        )
        doc = await repo.get_by_id(doc_id, user_email="anyone@co.com")
        assert doc is not None

    @pytest.mark.asyncio
    async def test_user_in_users_list(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": False, "users": ["alice@co.com"], "groups": []}
        )
        doc = await repo.get_by_id(doc_id, user_email="alice@co.com")
        assert doc is not None

    @pytest.mark.asyncio
    async def test_user_not_in_users_list(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": False, "users": ["alice@co.com"], "groups": []}
        )
        doc = await repo.get_by_id(doc_id, user_email="bob@co.com")
        assert doc is None

    @pytest.mark.asyncio
    async def test_user_in_groups_list(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": False, "users": [], "groups": ["eng@co.com"]}
        )
        doc = await repo.get_by_id(doc_id, user_email="eng@co.com")
        assert doc is not None

    @pytest.mark.asyncio
    async def test_user_not_in_groups_list(self, db_pool, user_id, repo):
        doc_id = await _insert_doc(
            db_pool, user_id, {"public": False, "users": [], "groups": ["eng@co.com"]}
        )
        doc = await repo.get_by_id(doc_id, user_email="sales@co.com")
        assert doc is None

    @pytest.mark.asyncio
    async def test_nonexistent_doc_returns_none(self, repo):
        doc = await repo.get_by_id("nonexistent", user_email="alice@co.com")
        assert doc is None
