namespace OrbCausticProof

abbrev PublicBits : Nat := 64
abbrev InternalBits : Nat := 128

abbrev PublicIndex := Fin PublicBits
abbrev CodeIndex := Fin InternalBits

abbrev PublicId := PublicIndex → Bool
abbrev Codeword := CodeIndex → Bool

def liftPublic (index : PublicIndex) : CodeIndex :=
  ⟨index.1, Nat.lt_trans index.2 (by decide)⟩

def encode (message : PublicId) : Codeword := fun index =>
  if h : index.1 < PublicBits then
    message ⟨index.1, h⟩
  else
    false

def publicProjection (codeword : Codeword) : PublicId := fun index =>
  codeword (liftPublic index)

theorem publicProjection_encode (message : PublicId) :
    publicProjection (encode message) = message := by
  funext index
  simp [publicProjection, encode, liftPublic, index.2]

theorem encode_injective : Function.Injective encode := by
  intro left right equality
  have projectedEquality := congrArg publicProjection equality
  simpa [publicProjection_encode] using projectedEquality

def SignsAgreeOnPublic (message : PublicId) (observed : Codeword) : Prop :=
  ∀ index : PublicIndex, observed (liftPublic index) = message index

def primaryDecode (observed : Codeword) : PublicId :=
  publicProjection observed

theorem primaryDecode_of_signAgreement
    {message : PublicId}
    {observed : Codeword}
    (agreement : SignsAgreeOnPublic message observed) :
    primaryDecode observed = message := by
  funext index
  exact agreement index

def flipAt (message : PublicId) (target : PublicIndex) : PublicId := fun index =>
  if index = target then
    !(message index)
  else
    message index

def flipAtPair (message : PublicId) (left right : PublicIndex) : PublicId :=
  flipAt (flipAt message left) right

def twoFlipEnvelope (primary : PublicId) (lowConfidence : PublicIndex → Prop) : PublicId → Prop :=
  fun candidate =>
    candidate = primary
      ∨ (∃ index, lowConfidence index ∧ candidate = flipAt primary index)
      ∨ (∃ left right,
          lowConfidence left
            ∧ lowConfidence right
            ∧ left ≠ right
            ∧ candidate = flipAtPair primary left right)

theorem truth_in_twoFlipEnvelope_zero
    {primary : PublicId}
    {lowConfidence : PublicIndex → Prop} :
    twoFlipEnvelope primary lowConfidence primary := by
  exact Or.inl rfl

theorem truth_in_twoFlipEnvelope_one
    {primary truth : PublicId}
    {lowConfidence : PublicIndex → Prop}
    {index : PublicIndex}
    (confidence : lowConfidence index)
    (truthEq : truth = flipAt primary index) :
    twoFlipEnvelope primary lowConfidence truth := by
  exact Or.inr <| Or.inl ⟨index, confidence, truthEq⟩

theorem truth_in_twoFlipEnvelope_two
    {primary truth : PublicId}
    {lowConfidence : PublicIndex → Prop}
    {left right : PublicIndex}
    (leftConfidence : lowConfidence left)
    (rightConfidence : lowConfidence right)
    (distinct : left ≠ right)
    (truthEq : truth = flipAtPair primary left right) :
    twoFlipEnvelope primary lowConfidence truth := by
  exact Or.inr <| Or.inr ⟨left, right, leftConfidence, rightConfidence, distinct, truthEq⟩

end OrbCausticProof
