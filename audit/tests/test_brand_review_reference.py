import copy
from dataclasses import asdict, replace
import importlib.util
import json
from pathlib import Path
import random
import sys
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "brand_review_reference.py"
spec = importlib.util.spec_from_file_location("giveaway_reference", SOURCE)
r = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = r
spec.loader.exec_module(r)
VECTORS = SOURCE.parent / "schemas/giveaway-brand-review-v1.json"


def wallet(number, family="solana"):
    return ("0x" if family == "evm" else "") + number.to_bytes(20 if family == "evm" else 32, "big").hex()


def campaign(family="solana", **terms):
    return r.Campaign(r.HashProfile(family, wallet(1000, family), chain_id=31337, giveaway_id=7),
                      [r.Task(0, "Follow", "Follow the published brand account.", "https://example.invalid/brand",
                              "Public account identifier"),
                       r.Task(1, "Comment", "Publish an original comment.", "", "Public comment link")],
                      r.Terms(wallet(900, family), wallet(901, family), **terms))


def add_entry(c, number, salt="01", phrase="Chance", seal=True, reveal=True):
    owner = wallet(number, c.profile.family)
    c.publish(owner, 0, f"account-{number}", 110)
    c.publish(owner, 1, f"comment-{number}", 111)
    commitment = r.sha(phrase.encode() + b"\x1f" + bytes.fromhex(salt)).hex()
    c.commit(owner, commitment, 112)
    if seal:
        c.begin_seal(owner, 113)
        c.process_seal(owner, [0], 114)
        c.process_seal(owner, [1], 115)
        c.finish_seal(owner, 116)
        if reveal:
            c.reveal(owner, phrase, salt, 210)
    return owner


def ready(family="solana", count=4, **terms):
    c = campaign(family, **terms)
    for number in range(1, count + 1):
        add_entry(c, number)
    c.freeze(c.terms.host, 310)
    return c


def decide(c, approve, now=320, **kwargs):
    kwargs.setdefault("reason", 0 if approve else 1)
    pending = c.open_candidate(c.terms.host, now)
    c.review(c.terms.brand, pending["ordinal"], approve, now + 1, **kwargs)
    return pending


def history(family="solana", decisions=(False, True, False, True)):
    c = campaign(family)
    events, reveals = [], []
    for number in range(1, 5):
        owner = wallet(number, family)
        events.extend([
            {"operation": "publish", "wallet": owner, "task_id": 0, "text": f"account-{number}", "now": 110},
            {"operation": "publish", "wallet": owner, "task_id": 1, "text": f"comment-{number}", "now": 111},
            {"operation": "commit", "wallet": owner,
             "commitment": r.sha(b"Chance\x1f\x01").hex(), "now": 112},
            {"operation": "begin_seal", "wallet": owner, "now": 113},
            {"operation": "process_seal", "wallet": owner, "task_ids": [0], "now": 114},
            {"operation": "process_seal", "wallet": owner, "task_ids": [1], "now": 115},
            {"operation": "finish_seal", "wallet": owner, "now": 116},
        ])
        reveals.append({"operation": "reveal", "wallet": owner, "phrase": "Chance",
                        "salt_hex": "01", "now": 210})
    # Group calls by finalized time; transaction histories cannot travel backwards.
    events.sort(key=lambda event: event["now"])
    events.extend(reveals)
    events.append({"operation": "freeze", "signer": c.terms.host, "now": 310})
    for ordinal, approve in enumerate(decisions):
        events.extend([
            {"operation": "open_candidate", "signer": c.terms.host, "now": 320 + ordinal * 2},
            {"operation": "review", "signer": c.terms.brand, "ordinal": ordinal,
             "approve": approve, "now": 321 + ordinal * 2, "reason": 0 if approve else 1},
        ])
    if any(decisions):
        events.extend([{"operation": "prepare_outcome", "signer": c.terms.host, "now": 400},
                       {"operation": "lock", "signer": c.terms.host, "now": 401}])
    else:
        events.append({"operation": "refund", "now": 400})
    return {"profile": {"family": family, "instance": wallet(1000, family),
                        "chain_id": 31337, "giveaway_id": 7},
            "terms": asdict(c.terms), "tasks": [asdict(task) for task in c.tasks], "events": events}


class NativeHashes(unittest.TestCase):
    def test_abi_offsets_are_standard_and_independent(self):
        encoded = r.abi(7, "abc", "dé")
        self.assertEqual(int.from_bytes(encoded[32:64], "big"), 96)
        self.assertEqual(int.from_bytes(encoded[64:96], "big"), 160)
        self.assertEqual(encoded[128:131], b"abc")
        self.assertEqual(encoded[192:195], "dé".encode())
        self.assertEqual(len(encoded), 224)

    def test_native_profiles_are_explicitly_distinct(self):
        a, b = campaign("solana"), campaign("evm")
        self.assertNotEqual(a.task_root, b.task_root)
        self.assertNotEqual(a.profile.task_hash(a.tasks[0]), b.profile.task_hash(b.tasks[0]))

    def test_exact_utf8_bytes_no_normalization(self):
        self.assertEqual(r.text_bytes("é" * 128, 256), ("é" * 128).encode())
        for value in ("é" * 129, "\ud800", "\r\n\t ", "", "abc\0"):
            with self.subTest(value=repr(value)), self.assertRaises(r.Invalid):
                r.text_bytes(value, 256)
        self.assertNotEqual(r.sha("é".encode()), r.sha("e\u0301".encode()))

    def test_domain_and_record_binding(self):
        for family in ("solana", "evm"):
            c = campaign(family)
            h = c.profile.evidence_hash(wallet(1, family), 0, 1, "proof")
            for owner, task, version, text in ((wallet(2, family), 0, 1, "proof"),
                                              (wallet(1, family), 1, 1, "proof"),
                                              (wallet(1, family), 0, 2, "proof"),
                                              (wallet(1, family), 0, 1, "proof!")):
                self.assertNotEqual(h, c.profile.evidence_hash(owner, task, version, text))

    def test_alternate_address_spelling_cannot_duplicate_a_wallet(self):
        p = campaign("evm").profile
        with self.assertRaises(r.Invalid):
            p.wallet(wallet(1000, "evm").upper())

    def test_ordered_merkle_padding_and_leaf_double_hash(self):
        p = campaign("evm").profile
        leaf = p.winner_leaf(0, wallet(1, "evm"), 10, bytes.fromhex("11" * 32), 3, 1)
        self.assertEqual(len(leaf), 32)
        a, b, c = [r.sha(bytes([i])) for i in range(3)]
        self.assertEqual(r.merkle_root([a, b, c]), r.sha(r.sha(a + b) + r.sha(c + r.ZERO)))
        self.assertNotEqual(r.merkle_root([a, b]), r.merkle_root([b, a]))


class EvidenceAndPool(unittest.TestCase):
    def test_missing_duplicate_unknown_task_cannot_seal(self):
        c = campaign()
        owner = wallet(1)
        c.publish(owner, 0, "proof", 110)
        c.commit(owner, r.sha(b"Chance\x1f\x01").hex(), 111)
        c.begin_seal(owner, 112)
        for ids in ([0, 0], [1], [0, 1], [0, 1, 2], []):
            with self.subTest(ids=ids), self.assertRaises(r.Invalid):
                c.process_seal(owner, ids, 113)
        self.assertIsNone(c.entries[owner].index)
        with self.assertRaises(r.Invalid):
            c.finish_seal(owner, 114)
        with self.assertRaises(r.Invalid):
            c.publish(owner, 2, "unknown", 115)

    def test_edit_invalidates_seal_and_excludes_latest_incomplete_revision(self):
        c = campaign()
        a = add_entry(c, 1, reveal=False)
        b = add_entry(c, 2)
        c.publish(a, 0, "new current revision", 119)
        c.begin_seal(a, 120)
        c.process_seal(a, [0], 121)
        c.publish(a, 1, "revised", 122)
        with self.assertRaises(r.Invalid):
            c.process_seal(a, [1], 123)
        with self.assertRaises(r.Invalid):
            c.attest(a, 210)
        self.assertEqual(c.entries[a].index, 0)
        c.freeze(c.terms.host, 310)
        self.assertEqual(c.ranking, [b])
        self.assertEqual(c.entries[b].index, 1)

    def test_drafts_do_not_reserve_participant_capacity(self):
        c = campaign(participant_cap=1)
        add_entry(c, 1, seal=False)
        b = add_entry(c, 2)
        self.assertEqual(c.registered, [b])
        a = wallet(1)
        c.begin_seal(a, 120)
        c.process_seal(a, [0, 1], 121)
        with self.assertRaises(r.Invalid):
            c.finish_seal(a, 122)

    def test_evidence_revision_cap_and_cutoff(self):
        c = campaign()
        a = add_entry(c, 1, reveal=False)
        for i in range(3):
            c.publish(a, 0, f"revision {i}", 130)
        with self.assertRaises(r.Invalid):
            c.publish(a, 0, "fifth", 130)
        c.begin_seal(a, 199)
        c.process_seal(a, [0, 1], 199)
        with self.assertRaises(r.Invalid):
            c.finish_seal(a, 200)

    def test_salt_evidence_and_final_approval_count_do_not_change_ranking(self):
        for family in ("solana", "evm"):
            a, b = campaign(family), campaign(family)
            for number in range(1, 5):
                add_entry(a, number, salt="01")
                owner = add_entry(b, number, salt="02", reveal=False)
                b.publish(owner, 0, "Different public evidence", 120)
                b.begin_seal(owner, 121)
                b.process_seal(owner, [0, 1], 122)
                b.finish_seal(owner, 123)
                b.reveal(owner, "Chance", "02", 210)
            a.freeze(a.terms.host, 310)
            b.freeze(b.terms.host, 310)
            self.assertEqual((a.pool, a.seed, a.ranking), (b.pool, b.seed, b.ranking))
            original = a.seed
            for approve in (False, True, False, True):
                decide(a, approve)
            self.assertEqual(a.seed, original)
            self.assertEqual(len(a.approved), 2)

    def test_missing_accepted_reveal_and_early_host_publication(self):
        c = campaign()
        a = add_entry(c, 1, reveal=False)
        add_entry(c, 2, reveal=False)
        c.attest(a, 210)
        with self.assertRaises(r.Invalid):
            c.reveal(a, "Chance", "01", 211, signer=c.terms.host)
        with self.assertRaises(r.Invalid):
            c.freeze(c.terms.host, 310)
        c.refund(400)
        self.assertEqual(c.outcome["reason"], "missing_accepted_reveal")


class DecisionsAndAccounting(unittest.TestCase):
    def test_brand_is_final_and_no_objective_proof_is_required(self):
        c = ready()
        a = decide(c, False, reason=1, note="Brand assessment")
        b = decide(c, True)
        d = decide(c, False)
        e = decide(c, True)
        self.assertEqual([a["slot"], b["slot"], d["slot"], e["slot"]], [0, 0, 1, 1])
        self.assertEqual(c.approved, [c.ranking[1], c.ranking[3]])
        with self.assertRaises(r.Invalid):
            c.pay(c.approved[0])
        c.prepare_outcome(c.terms.host, 400)
        c.lock(c.terms.host, 401)
        self.assertEqual(c.outcome["per_winner"], 4751)
        self.assertEqual(c.outcome["creator_refund"], 1)
        with self.assertRaises(r.Invalid):
            c.pay(c.ranking[0])
        self.assertEqual(c.pay(c.approved[0]), 4751)
        with self.assertRaises(r.Invalid):
            c.pay(c.approved[0])
        with self.assertRaises(r.Invalid):
            c.refund(10000)
        self.assertEqual(c.pay(c.approved[1]), 4751)

    def test_roles_replay_and_pending_candidate_no_skip(self):
        c = ready()
        with self.assertRaises(r.Invalid):
            c.open_candidate(c.terms.brand, 320)
        p = c.open_candidate(c.terms.host, 320)
        with self.assertRaises(r.Invalid):
            c.open_candidate(c.terms.host, 321)
        for signer, ordinal in ((c.terms.host, 0), (c.terms.brand, 1)):
            with self.assertRaises(r.Invalid):
                c.review(signer, ordinal, True, 322)
        c.review(c.terms.brand, p["ordinal"], True, 323)
        with self.assertRaises(r.Invalid):
            c.review(c.terms.brand, p["ordinal"], False, 324)
        with self.assertRaises(r.Invalid):
            c.prepare_outcome(c.terms.host, 325)

    def test_review_deadline_is_strict_and_no_automatic_verdict(self):
        c = ready()
        p = c.open_candidate(c.terms.host, 320)
        with self.assertRaises(r.Invalid):
            c.refund(p["review_deadline"] - 1)
        with self.assertRaises(r.Invalid):
            c.review(c.terms.brand, 0, False, p["review_deadline"])
        c.refund(p["review_deadline"])
        self.assertEqual(c.decisions, [])
        self.assertEqual(c.outcome["reason"], "review_expiry")
        self.assertEqual(c.outcome["creator_refund"], c.terms.funding)

    def test_cutoff_does_not_shorten_last_review_or_refund_pending(self):
        c = ready(count=1, winners=1)
        p = c.open_candidate(c.terms.host, 499)
        self.assertEqual(p["review_deadline"], 599)
        with self.assertRaises(r.Invalid):
            c.refund(500)
        c.review(c.terms.brand, 0, True, 598)
        c.prepare_outcome(c.terms.host, 600)
        c.lock(c.terms.host, 649)
        self.assertEqual(c.outcome["kind"], "success")

    def test_unresolved_cutoff_is_not_pool_exhaustion(self):
        c = ready()
        decide(c, True)
        with self.assertRaises(r.Invalid):
            c.open_candidate(c.terms.host, 500)
        c.refund(500)
        self.assertEqual(c.outcome["reason"], "opening_cutoff_unresolved")
        self.assertEqual(len(c.approved), 1)
        self.assertEqual(c.outcome["winners"], [])

    def test_partial_approved_exhaustion_redistributes_entire_pool(self):
        c = ready(count=2)
        decide(c, False)
        decide(c, True)
        with self.assertRaises(r.Invalid):
            c.refund(500)
        c.prepare_outcome(c.terms.host, 501)
        c.lock(c.terms.host, 502)
        self.assertEqual(c.outcome["per_winner"], 9503)
        self.assertEqual(c.outcome["creator_refund"], 0)

    def test_all_rejected_or_empty_pool_refund_without_fee(self):
        for count in (0, 4):
            c = ready(count=count)
            for _ in range(count):
                decide(c, False)
            c.refund(400)
            self.assertEqual(c.outcome["reason"], "zero_approved_exhaustion")
            self.assertEqual(c.outcome["fee"], 0)

    def test_preparation_does_not_vest_and_absolute_expiry_wins_boundary(self):
        c = ready(count=1, winners=1)
        decide(c, True)
        c.prepare_outcome(c.terms.host, 400)
        with self.assertRaises(r.Invalid):
            c.lock(c.terms.host, 650)
        c.refund(650)
        self.assertEqual(c.outcome["reason"], "absolute_expiry")
        with self.assertRaises(r.Invalid):
            c.pay(c.approved[0])

    def test_accounting_conservation_randomized_and_large_values(self):
        rng = random.Random(7)
        for _ in range(100):
            count = rng.randrange(1, 20)
            funding = rng.randrange(10**12, 2**128)
            fee_bps = rng.randrange(10000)
            c = ready(count=count, winners=count, funding=funding, fee_bps=fee_bps)
            for i in range(count):
                decide(c, True, now=320 + i)
            c.prepare_outcome(c.terms.host, 400)
            c.lock(c.terms.host, 401)
            o = c.outcome
            self.assertEqual(o["fee"] + o["per_winner"] * count + o["creator_refund"], funding)
            self.assertLess(o["creator_refund"], count)


class GoldenHistories(unittest.TestCase):
    def test_vectors_replay_and_hashes_match_frozen_file(self):
        vectors = json.loads(VECTORS.read_text())
        self.assertEqual(vectors["schema"], "giveaway-contract-conformance-v2")
        for vector in vectors["histories"]:
            with self.subTest(vector=vector["name"]):
                self.assertEqual(r.replay(vector["input"]), vector["expected"])
        for vector in vectors["hash_vectors"]:
            p = r.HashProfile(**vector["profile"])
            task = r.Task(**vector["task"])
            self.assertEqual(p.task_hash(task).hex(), vector["task_hash"])
            self.assertEqual(p.task_root([task]).hex(), vector["task_root"])
            self.assertEqual(p.evidence_hash(vector["wallet"], 0, 1, "proof").hex(), vector["evidence_hash"])
            self.assertEqual(p.phrase_digest(vector["wallet"], b"Chance").hex(), vector["phrase_digest"])
            c = ready(vector["profile"]["family"])
            self.assertEqual(c.pool.hex(), vector["pool"])
            self.assertEqual(c.seed.hex(), vector["seed"])
            self.assertEqual([{"wallet": w, "key": p.candidate_key(c.seed, w).hex()}
                              for w in c.ranking], vector["candidate_keys"])
            if p.family == "solana":
                root = p.accepted_root(vector["winners"])
            else:
                root = r.merkle_root([p.winner_leaf(i, winner["wallet"], 4751,
                                     bytes.fromhex(winner["evidence_root"]), winner["revision"],
                                     winner["ordinal"])
                                     for i, winner in enumerate(vector["winners"])])
            self.assertEqual(root.hex(), vector["winner_root"])
            self.assertEqual(p.outcome_hash(bytes.fromhex(vector["outcome_rules_hash"]), c.seed,
                                           root, 2, 4751, 500, 1, 4).hex(), vector["outcome_hash"])

    def test_replay_rejects_omitted_or_altered_decisions_and_reordered_coverage(self):
        source = history()
        for change in ("omit", "wrong-ordinal", "duplicate-task", "wrong-brand"):
            document = copy.deepcopy(source)
            if change == "omit":
                document["events"].pop(34)
            elif change == "wrong-ordinal":
                document["events"][34]["ordinal"] = 1
            elif change == "duplicate-task":
                event = next(e for e in document["events"] if e["operation"] == "process_seal")
                event["task_ids"] = [0, 0]
            else:
                document["events"][34]["signer"] = wallet(901)
            with self.subTest(change=change), self.assertRaises(r.Invalid):
                r.replay(document)

    def test_claimed_snapshot_rejects_changed_root_fee_or_winner(self):
        document = history()
        expected = r.replay(document)
        for change in ("root", "fee", "winner"):
            tampered = copy.deepcopy(document)
            tampered["claimed_snapshot"] = copy.deepcopy(expected)
            if change == "root":
                tampered["claimed_snapshot"]["task_root"] = "00" * 32
            elif change == "fee":
                tampered["claimed_snapshot"]["outcome"]["fee"] += 1
            else:
                tampered["claimed_snapshot"]["outcome"]["winners"][0] = wallet(99)
            with self.subTest(change=change), self.assertRaises(r.Invalid):
                r.replay(tampered)

    def test_observed_candidate_cannot_skip_full_sort_order(self):
        document = history()
        event = next(e for e in document["events"] if e["operation"] == "open_candidate")
        event["observed"] = {"wallet": wallet(99), "ordinal": 0}
        with self.assertRaises(r.Invalid):
            r.replay(document)

    def test_pending_native_consumption_counters_have_explicit_meanings(self):
        for family in ("solana", "evm"):
            document = history(family)
            document["events"] = document["events"][:34]
            document["native_counters"] = {"registered": 4, "approved": 0, "opened": 1,
                                            "terminal": 0,
                                            "native_consumed": 1 if family == "evm" else 0}
            self.assertIsNotNone(r.replay(document)["pending"])
            document["native_counters"]["native_consumed"] = 9
            with self.assertRaises(r.Invalid):
                r.replay(document)


if __name__ == "__main__":
    unittest.main()
