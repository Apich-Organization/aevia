"""Aevia RSSN domain replay plans used by contracts and release packaging."""

# Each suite belongs to contracts, release smoke, or both gates.
_CONTRACTS = 1
_SMOKE = 2
_BOTH = _CONTRACTS + _SMOKE

_SUITES = {
    "acoustic_storage": _SMOKE,
    "analytic_continuation": _CONTRACTS,
    "cad_mesh": _BOTH,
    "complex_ball_certification": _BOTH,
    "complex_branch_cuts": _CONTRACTS,
    "complex_coordinate_maps": _BOTH,
    "complex_disk": _CONTRACTS,
    "complex_dynamics": _CONTRACTS,
    "complex_elliptic": _BOTH,
    "complex_extended_special": _BOTH,
    "complex_orthogonal": _CONTRACTS,
    "complex_polynomial": _CONTRACTS,
    "complex_root_contours": _CONTRACTS,
    "complex_root_isolation": _BOTH,
    "complex_root_iterations": _BOTH,
    "conformal_kernels": _BOTH,
    "conformal_mobius": _CONTRACTS,
    "conformal_stencil": _BOTH,
    "conformal_szego": _BOTH,
    "contour_compatibility": _BOTH,
    "contour_deformation": _BOTH,
    "contour_geometry": _CONTRACTS,
    "contour_quadrature": _CONTRACTS,
    "control_mpc": _BOTH,
    "control_trajectories": _BOTH,
    "control_transfer": _BOTH,
    "dense_economy": _BOTH,
    "dynamics_callbacks": _CONTRACTS,
    "finite_field_ntt": _BOTH,
    "glm_links": _BOTH,
    "holonomic_borel_laurent": _BOTH,
    "interpolation": _BOTH,
    "hyperelastic_observer": _SMOKE,
    "krylov_csr": _BOTH,
    "linear_programming": _BOTH,
    "mcmc": _BOTH,
    "modular": _BOTH,
    "mp_bessel": _BOTH,
    "mp_complex_exp_log": _BOTH,
    "mp_complex_inverse": _BOTH,
    "mp_complex_root_power": _BOTH,
    "mp_complex_trigonometry": _BOTH,
    "mp_constants": _CONTRACTS,
    "mp_decimal": _CONTRACTS,
    "mp_dirichlet": _BOTH,
    "mp_enclosures": _BOTH,
    "mp_error_functions": _BOTH,
    "mp_exact_roots": _CONTRACTS,
    "mp_gamma_family": _BOTH,
    "mp_hyperbolic": _CONTRACTS,
    "mp_inverse_sine": _CONTRACTS,
    "mp_inverse_tangent": _CONTRACTS,
    "mp_logarithms": _CONTRACTS,
    "mp_powers": _CONTRACTS,
    "mp_trigonometry": _CONTRACTS,
    "multiprecision": _BOTH,
    "native_split_step": _SMOKE,
    "numerical_transforms": _BOTH,
    "optimizer": _BOTH,
    "parallel_quadrature": _BOTH,
    "planar_meshgen": _BOTH,
    "prepared_state_space": _CONTRACTS,
    "real_polynomial": _BOTH,
    "riemann_covering": _BOTH,
    "riemann_hilbert": _BOTH,
    "riemann_hilbert_scalar": _BOTH,
    "schwarz_christoffel_solve": _BOTH,
    "sparse_exact": _BOTH,
    "sparse_sqp": _BOTH,
    "state_space_diagnostics": _CONTRACTS,
    "saddle": _BOTH,
    "statrs": _CONTRACTS,
    "survival_models": _BOTH,
    "univalent": _BOTH,
    "univalent_sampled": _BOTH,
}

_BACKENDS = ["--backend=ast", "--backend=ir"]

RSSN_CONTRACT_PLAN = _BACKENDS + [
    "--example=" + example
    for example in ["sensor_calibration", "relaxation_fit", "complex_special_functions"]
] + [
    "--suite=" + suite
    for suite, gate in _SUITES.items()
    if gate in [_CONTRACTS, _BOTH]
]

RSSN_RELEASE_PLAN = _BACKENDS + [
    "--suite=" + suite
    for suite, gate in _SUITES.items()
    if gate in [_SMOKE, _BOTH]
]
