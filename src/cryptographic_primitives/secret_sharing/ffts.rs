use crate::arithmetic::{Converter, Modulo};
use crate::BigInt;
use crate::{
    cryptographic_primitives::secret_sharing::Polynomial,
    elliptic::curves::{Scalar, Secp256k1},
};
use std::iter::IntoIterator;
use std::iter::Iterator;
use std::vec::IntoIter;

/// Iterator for powers of a given element
///
/// For a given element $g$ and a non-negative number $c$, the iterator yields $g^0,g^1,\ldots,g^{c-1}$
struct PowerIterator {
    base: Scalar<Secp256k1>,
    next_pow: Scalar<Secp256k1>,
    next_idx: usize,
    max_idx: usize,
}

impl PowerIterator {
    pub fn new(base: Scalar<Secp256k1>, count: usize) -> Self {
        PowerIterator {
            base,
            next_pow: Scalar::<Secp256k1>::from(1),
            next_idx: 0,
            max_idx: count,
        }
    }
}
impl Iterator for PowerIterator {
    type Item = Scalar<Secp256k1>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_idx == self.max_idx {
            return None;
        }
        let res = self.next_pow.clone();
        self.next_pow = &res * self.base.clone();
        self.next_idx += 1;
        Some(res)
    }
}
/// Iterator for all facorizations of a given number.
///
/// For a given number $s$ and its factorization $s=p_1^k_1 \cdot p_2^k_2 \cdot \ldots \cdot p_n^k_n$
/// this iterator yields all divisors of $s$.
struct FactorizationIterator<'a> {
    factorization: &'a [(usize, usize)],
    index: usize,
    max: usize,
}

// "115481771728459905245102424859900657047113141323743738905491223467302634970004" - of degree 18051648
// This is the biggest "small" factor of q-1.
const PRIMITIVE_ROOT_OF_UNITY: &str =
    "115481771728459905245102424859900657047113141323743738905491223467302634970004";
const ROOT_OF_UNITY_BASIC_ORDER: usize = 18051648;

// This is the factorization of [ROOT_OF_UNITY_BASIC_ORDER]
const FACTORIZATION_OF_ORDER: [(usize, usize); 4] = [(2, 6), (3, 1), (149, 1), (631, 1)];

impl<'a> FactorizationIterator<'a> {
    fn new(factors: &'a [(usize, usize)]) -> FactorizationIterator {
        let max = factors
            .iter()
            .fold(1usize, |acc, (_, count)| acc * (count + 1));
        FactorizationIterator {
            factorization: factors,
            index: 0usize,
            max,
        }
    }
}

impl<'a> Iterator for FactorizationIterator<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        let mut index = self.index;
        if self.index >= self.max {
            return None;
        }
        #[allow(unused_parens)]
        let output = self
            .factorization
            .iter()
            .fold(1usize, |acc, (factor, count)| {
                let how_many = index % (count + 1);
                index /= (count + 1);
                acc * factor.pow(how_many as u32)
            });
        self.index += 1;
        Some(output)
    }
}

struct ModularSliceIterator<'a, T> {
    slice: &'a [T],
    step: usize,
    next_index: usize,
}

impl<'a, T> ModularSliceIterator<'a, T> {
    fn new(slice: &'a [T], step: usize) -> Self {
        ModularSliceIterator {
            slice,
            step,
            next_index: 0,
        }
    }
}

impl<'a, T> Iterator for ModularSliceIterator<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.next_index == self.slice.len() {
            return None;
        }
        let res = Some(
            self.slice
                .get((self.step * (self.next_index)) % self.slice.len())
                .unwrap(),
        );
        self.next_index += 1;
        res
    }
}

fn dot_product<'a>(
    a: impl IntoIterator<Item = &'a Scalar<Secp256k1>>,
    b: impl IntoIterator<Item = &'a Scalar<Secp256k1>>,
) -> Scalar<Secp256k1> {
    a.into_iter().zip(b.into_iter()).map(|(i, j)| i * j).sum()
}
// Factors a number using a set of given factors
fn obtain_factorization(num: usize, factors: &[usize]) -> Option<Vec<(usize, usize)>> {
    let mut num_left = num;
    let ret_val = Some(
        factors
            .iter()
            .map(|factor| {
                let mut count = 0usize;
                while num_left % factor == 0 {
                    num_left /= factor;
                    count += 1;
                }
                (*factor, count)
            })
            .filter(|&(_, count)| count != 0)
            .collect(),
    );
    if num_left != 1 {
        None
    } else {
        ret_val
    }
}

// Given a factorization of a number $n$ and a lower bar $b$, find the smallest $m$ such that $m|n$ and $m>b$
fn find_minimal_factorization_bigger_than(
    low_bar: usize,
    factorization: &[(usize, usize)],
) -> Option<Vec<(usize, usize)>> {
    let mut best_divisor = usize::MAX;
    for divisor in FactorizationIterator::new(factorization) {
        if divisor > low_bar && divisor < best_divisor {
            best_divisor = divisor;
        }
    }

    obtain_factorization(
        best_divisor,
        &factorization
            .iter()
            .map(|(factor, _)| *factor)
            .collect::<Vec<usize>>(),
    )
}

fn obtain_split_factor(factors: &[(usize, usize)], mut factor_index: usize) -> Option<usize> {
    for &(factor, count) in factors {
        if factor_index < count {
            return Some(factor);
        }
        factor_index -= count;
    }
    None
}

fn merge_polynomials(
    polynomials: Vec<Polynomial<Secp256k1>>,
    fft_size: usize,
) -> Polynomial<Secp256k1> {
    let polynomials_length = polynomials.len();
    let mut iters: Vec<IntoIter<Scalar<Secp256k1>>> = polynomials
        .into_iter()
        .map(|p| p.into_coefficients().into_iter())
        .collect();
    Polynomial::<Secp256k1>::from_coefficients(
        (0..fft_size)
            .map(|i| {
                iters[i % polynomials_length]
                    .next()
                    .unwrap_or_else(Scalar::<Secp256k1>::zero)
            })
            .collect(),
    )
}
fn split_polynomial(
    polynomial: Polynomial<Secp256k1>,
    factor: usize,
) -> Vec<Polynomial<Secp256k1>> {
    let mut coefficient_vectors = vec![Vec::new(); factor];
    polynomial
        .into_coefficients()
        .into_iter()
        .enumerate()
        .for_each(|(i, coefficient)| coefficient_vectors[i % factor].push(coefficient));
    coefficient_vectors
        .into_iter()
        .map(Polynomial::<Secp256k1>::from_coefficients)
        .collect()
}

// Evaluates the polynomial on all the fft_size powers of the generator.
// Folds the recursion step with factor number factor_index from the size_factorization.
fn fft_internal(
    polynomial: Polynomial<Secp256k1>,
    generator: BigInt,
    size_factorization: &[(usize, usize)],
    fft_size: usize,
    factor_index: usize,
) -> Vec<Scalar<Secp256k1>> {
    let split_factor = obtain_split_factor(size_factorization, factor_index);
    let generator_scalar = Scalar::<Secp256k1>::from_bigint(&generator);
    match split_factor {
        None => PowerIterator::new(generator_scalar, fft_size)
            .into_iter()
            .map(|root| polynomial.evaluate(&root))
            .collect(),
        Some(split_factor) => {
            let post_split_generator = Scalar::<Secp256k1>::from_bigint(&BigInt::mod_pow(
                &generator,
                &BigInt::from(split_factor as u64),
                Scalar::<Secp256k1>::group_order(),
            ));
            let split_polys = split_polynomial(polynomial, split_factor);
            let evals: Vec<Vec<Scalar<Secp256k1>>> = split_polys
                .into_iter()
                .map(|sub_poly| {
                    fft_internal(
                        sub_poly,
                        post_split_generator.to_bigint(),
                        size_factorization,
                        fft_size / split_factor,
                        factor_index + 1,
                    )
                })
                .collect();
            PowerIterator::new(generator_scalar, fft_size)
                .into_iter()
                .enumerate()
                .map(|(idx, eval_item)| {
                    PowerIterator::new(eval_item, split_factor)
                        .into_iter()
                        .enumerate()
                        .map(|(degree, cur_item_degree)| {
                            cur_item_degree
                                * &evals[degree][((idx * split_factor) % fft_size) / split_factor]
                        })
                        .fold(Scalar::<Secp256k1>::zero(), |acc, cur| acc + cur)
                })
                .collect()
        }
    }
}

pub fn fft(polynomial: Polynomial<Secp256k1>, fft_size: usize) -> Vec<Scalar<Secp256k1>> {
    let factors_to_expand = obtain_factorization(
        fft_size,
        &FACTORIZATION_OF_ORDER
            .iter()
            .map(|&(f, _)| f)
            .collect::<Vec<usize>>(),
    )
    .expect("Given degree doesn't divide order of primitive root-of-unity");
    let fft_generator = BigInt::mod_pow(
        &BigInt::from_str_radix(PRIMITIVE_ROOT_OF_UNITY, 10)
            .expect("Failed to decode primitive root of unitiy"),
        &BigInt::from((ROOT_OF_UNITY_BASIC_ORDER / fft_size) as u64),
        Scalar::<Secp256k1>::group_order(),
    );
    fft_internal(polynomial, fft_generator, &factors_to_expand, fft_size, 0)
}

fn inverse_fft_internal(
    fft_vec: Vec<Scalar<Secp256k1>>,
    fft_size_factorization: &[(usize, usize)],
    fft_split_factor_index: usize,
    primitive_root_of_unity: &BigInt,
) -> Polynomial<Secp256k1> {
    // ---------------------------------------------------------------------------------
    // -------------------- Algorithm description in a nutshell (*) --------------------
    // ---------------------------------------------------------------------------------
    //  (*) - I really hope this will be a nutshell... let's see.
    //
    // So at this point we're looking for the coefficients of polynomial P(x) such that:
    //                  P(g^i) = fft_vec[i]
    // Where g is a root of unity of order of n=fft_vec.len().
    // The polynomial P(x) has the form:
    //                         n-1
    //                  P(x) = Sum a_ix^i
    //                         i=0
    // Let's say n has a small prime factor d.
    // i.e.: n=d*k for some integer k.
    // We can also write P(x) like this:
    //        d-1  k-1
    // P(x) = Sum {Sum a_{i+(j*d)}*x^{i+(j*d)}} =
    //        i=0  j=0
    // -------------------------------------------
    //        d-1      k-1
    //      = Sum {x^i Sum {a_{i+(j*d)}*x^{j*d}}}
    //        i=0      j=0
    //                                k-1
    // For each i we can denote each  Sum {a_{i+j*d} * x^{j*d}} as a polynomial P_i(x).
    //                                j=0
    //                            k-1
    // We can also write P_i(x) = Sum {a_{i+j*d} * (x^d)*j}.
    //                            j=0
    // By making the substitution y=x^d we can think of P_i(y) as the following:
    //          k-1
    // P_i(y) = Sum {a_{i+j*d} * y^j} which is a polynomial of degree k.
    //          j=0
    //
    // Coroallary #1:
    // Had we known the coefficients of all P_i(y) we could very simply derive the
    // coefficients of P(x) = sum a_ix^i, since each coefficient of each P_i(y) is
    // algebraically equal to a coefficient of P(x).
    //
    // Recall we have fft_vec, a vector of evaluations of P(x) on a set of powers of
    // a primitive root-of-unity of order n.
    // From those, we can derive the evaluation of P_i(x) for the powers of a
    // root-of-unity of order n/d.
    //
    // Let g denote the primitive root of unity of order n.
    // Let h=g^d, be the primitive root of unity of order n/d.
    // Let h^i for some 0<=i<n/d be a power of h.
    // Thus, there are d powers of g: g^{i_1},...,g^{i_d} such that
    // (g^{i_1})^d = ... = g^{i_d}^d = h^i.
    // Thus the following equations hold:
    // Consider the following set of d equations:
    //
    // - P(g^{i_1}) = P_0(h) + g^{i_1}*P_1(h) + ... + (g^{i_1})^{d-1})*P_{d-1}(h)
    // - P(g^{i_2}) = P_0(h) + g^{i_2}*P_1(h) + ... + (g^{i_2})^{d-1})*P_{d-1}(h)
    //   ........................................................................
    // - P(g^{i_d}) = P_0(h) + g^{i_d}*P_1(h) + ... + (g^{i_d})^{d-1})*P_{d-1}(h)
    //
    // This set of equations can also be written using matrix/vector notation:
    // (P(g^{i_1}))      (1  g^{i_1}   (g^{i_1})^2 .... (g^{i_1})^{d-1})   (  P_0(h^i)  )
    // (P(g^{i_2}))      (1  g^{i_2}   (g^{i_2})^2 .... (g^{i_2})^{d-1})   (  P_1(h^i)  )
    // (..........) ==== (.............................................) X (............)
    // (..........)      (.............................................)   (............)
    // (P(g^{i_d}))      (1  g^{i_d}   (g^{i_d})^2 .... (g^{i_d})^{d-1})   (P_{d-1}(h^i))
    // \____ ____/       \_______________________  ___________________/    \_____ ____/
    //      V                                    v                               v
    //      A                                    B                               C
    //
    // We know vector A (since the evaluations are given in fft_vec).
    // We can invert B (it's a vandermonde's matrix).
    // So we can compute C = B^{-1} * A
    // By doing so for h^i for all 0<=i<n/d we can obtain the evaluations of
    // P_0,...,P_{d-1} of degree n/d over n/d powers of primitive root of unity h
    // So we can recusively obtain the coefficient-representation of P_0,...,P_{d-1}.
    // From those representations we can obtain the coefficient-representation of P(x)
    // (based on Corollary #1).

    let split_factor = obtain_split_factor(fft_size_factorization, fft_split_factor_index);
    let fft_size = fft_vec.len();
    match split_factor {
        None => Polynomial::from_coefficients(fft_vec),
        Some(split_factor) => {
            let post_split_fft_size = fft_size / split_factor;
            let post_split_fft_generator = BigInt::mod_pow(
                primitive_root_of_unity,
                &BigInt::from(split_factor as u64),
                Scalar::<Secp256k1>::group_order(),
            );
            // TODO: The following line can be computed once per fft-recursion-level.
            let inverse_dft_generator_powers: Vec<Scalar<Secp256k1>> = PowerIterator::new(
                Scalar::<Secp256k1>::from_bigint(&BigInt::mod_pow(
                    primitive_root_of_unity,
                    &BigInt::from((post_split_fft_size * (split_factor - 1)) as u64),
                    Scalar::<Secp256k1>::group_order(),
                )),
                split_factor,
            )
            .collect();
            let split_factor_inverse =
                Scalar::<Secp256k1>::from_bigint(&BigInt::from(split_factor as u64))
                    .invert()
                    .unwrap();

            // For each power 'h^i' of the post-split generator 'h' we find 'split_factor' powers of
            // pre-split generator 'g' who are 'split_factor'-roots of 'h^i'.
            // These are powers of 'g', 'g^d' such that (d*split_factor=i*split_factor) mod (fft_size)
            // Since we look for 'split_factor' such 'd's : d_0,...,d_{split_factor-1} we can see that for:
            let mut a_vecs = vec![Vec::with_capacity(split_factor); post_split_fft_size];

            // bye bye fft_vec
            fft_vec.into_iter().enumerate().for_each(|(i, e)| {
                a_vecs[i % post_split_fft_size].push(e);
            });

            let mut sub_ffts: Vec<Vec<Scalar<Secp256k1>>> =
                vec![Vec::with_capacity(post_split_fft_size); split_factor];

            PowerIterator::new(
                Scalar::<Secp256k1>::from_bigint(primitive_root_of_unity),
                post_split_fft_size,
            )
            .enumerate()
            .for_each(|(i, w_l_i)| {
                // In this iteration we compute the evaluation of all 'split_factor' polynomials at point h_i.
                // Those are (g_i * (post_split_fft_generator^j)) for 0<=j<split_factor
                PowerIterator::new(w_l_i, split_factor)
                    .enumerate()
                    .for_each(|(j, w_l_ij)| {
                        // Compute P_i(h^i)
                        // Iterator over i-th inverse-DFT matrix
                        sub_ffts[j].push(
                            w_l_ij.invert().unwrap()
                                * &split_factor_inverse
                                * dot_product(
                                    ModularSliceIterator::new(&inverse_dft_generator_powers, j),
                                    &a_vecs[i],
                                ),
                        );
                    });
            });
            merge_polynomials(
                sub_ffts
                    .into_iter()
                    .map(|fft_vec_post| {
                        inverse_fft_internal(
                            fft_vec_post,
                            fft_size_factorization,
                            fft_split_factor_index + 1,
                            &post_split_fft_generator,
                        )
                    })
                    .collect(),
                fft_size,
            )
        }
    }
}
// evaluations[i] = P(g^i)
pub fn inverse_fft(evaluations: Vec<Scalar<Secp256k1>>) -> Polynomial<Secp256k1> {
    // Find the factorization of the length of the evaluation vector.
    let factorization = obtain_factorization(
        evaluations.len(),
        &FACTORIZATION_OF_ORDER
            .iter()
            .map(|(factor, _)| *factor)
            .collect::<Vec<usize>>(),
    )
    .expect("The size of the given FFT doesn't divide the order of the primitive-root-of-unity.");
    let fft_size = evaluations.len();
    let generator = BigInt::mod_pow(
        &BigInt::from_str_radix(PRIMITIVE_ROOT_OF_UNITY, 10).unwrap(),
        &BigInt::from((ROOT_OF_UNITY_BASIC_ORDER / fft_size) as u64),
        Scalar::<Secp256k1>::group_order(),
    );
    inverse_fft_internal(evaluations, &factorization, 0, &generator)
}

pub fn multiply_polynomials(
    a: Polynomial<Secp256k1>,
    b: Polynomial<Secp256k1>,
) -> Polynomial<Secp256k1> {
    let fft_size = find_minimal_factorization_bigger_than(
        (a.degree() + b.degree()).into(),
        &FACTORIZATION_OF_ORDER,
    )
    .expect("Degree of given polynomials is too big!")
    .iter()
    .fold(1, |acc, &(factor, count)| acc * (factor.pow(count as u32)));
    let fft_a = fft(a, fft_size);
    let fft_b = fft(b, fft_size);
    let fft_c = fft_a.iter().zip(fft_b).map(|(a, b)| a * b).collect();
    inverse_fft(fft_c)
}

#[cfg(test)]
mod tests {

    use crate::{
        arithmetic::{Converter, Modulo},
        cryptographic_primitives::secret_sharing::{
            ffts::{fft, inverse_fft},
            Polynomial,
        },
        elliptic::curves::{Scalar, Secp256k1},
        BigInt,
    };

    use super::{PowerIterator, PRIMITIVE_ROOT_OF_UNITY, ROOT_OF_UNITY_BASIC_ORDER};

    fn make_scalar(num: u32) -> Scalar<Secp256k1> {
        Scalar::<Secp256k1>::from_bigint(&BigInt::from(num))
    }

    fn get_primitive_root_of_unity_for_size(fft_size: usize) -> Option<Scalar<Secp256k1>> {
        if 0 != (ROOT_OF_UNITY_BASIC_ORDER % fft_size) {
            return None;
        }
        Some(Scalar::from_bigint(&BigInt::mod_pow(
            &BigInt::from_str_radix(PRIMITIVE_ROOT_OF_UNITY, 10).unwrap(),
            &BigInt::from((ROOT_OF_UNITY_BASIC_ORDER / fft_size) as u64),
            Scalar::<Secp256k1>::group_order(),
        )))
    }

    fn get_degrees_of_primitive_root_of_unity_for_size(
        fft_size: usize,
    ) -> Option<impl Iterator<Item = Scalar<Secp256k1>>> {
        // prou == primitive-root-of-unity
        let prou = match get_primitive_root_of_unity_for_size(fft_size) {
            None => return None,
            Some(prou) => prou,
        };
        Some(PowerIterator::new(prou, fft_size))
    }

    #[test]
    fn evaluate_zero_degree_polynomial() {
        let c = make_scalar(5);
        let p = Polynomial::from_coefficients(vec![c.clone()]);
        let evals = fft(p, 1);
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0], c);
    }

    #[test]
    fn interpolate_zero_degree_polynomial() {
        let c = make_scalar(5);
        let coeffs = vec![c.clone()];
        let interpolated_result = inverse_fft(coeffs);
        assert_eq!(interpolated_result.degree(), 0);
        assert_eq!(interpolated_result.coefficients()[0], c);
    }

    #[test]
    fn evaluate_one_degree_polynomial() {
        let coeffs = vec![make_scalar(0), make_scalar(1)];
        // p(x) = x --> Easy to check
        let p = Polynomial::from_coefficients(coeffs);
        let evals = fft(p, 2);

        assert_eq!(evals.len(), 2);
        get_degrees_of_primitive_root_of_unity_for_size(2)
            .unwrap()
            .enumerate()
            .for_each(|(idx, rou)| {
                assert_eq!(evals[idx], rou);
            });
    }

    #[test]
    fn interpolate_one_degree_polynomial() {
        let evals = vec![make_scalar(1), make_scalar(1)];
        let interpolated_result = inverse_fft(evals.clone());
        // assert_eq!(interpolated_result.degree(), 0);
        get_degrees_of_primitive_root_of_unity_for_size(2)
            .unwrap()
            .enumerate()
            .for_each(|(idx, rou)| assert_eq!(evals[idx], interpolated_result.evaluate(&rou)))
    }

    #[test]
    fn evaluation_high_degree() {
        let deg = 100;
        let coeffs = (0..(deg + 1)).map(|i| make_scalar(i)).collect();
        let p = Polynomial::from_coefficients(coeffs);
        let fft_deg = 3 * 149;
        let evals = fft(p.clone(), fft_deg);

        assert_eq!(evals.len(), fft_deg);
        get_degrees_of_primitive_root_of_unity_for_size(fft_deg)
            .unwrap()
            .enumerate()
            .for_each(|(idx, rou)| {
                assert_eq!(evals[idx], p.evaluate(&rou));
            });
    }

    #[test]
    fn interpolate_high_degree() {
        let fft_size = 64 * 3 as usize;
        let evals: Vec<Scalar<Secp256k1>> =
            (0..(fft_size)).map(|i| make_scalar(i as u32)).collect();
        let p = inverse_fft(evals.clone());
        get_degrees_of_primitive_root_of_unity_for_size(fft_size)
            .unwrap()
            .enumerate()
            .for_each(|(idx, rou)| assert_eq!(evals[idx], p.evaluate(&rou)))
    }

    #[test]
    fn ensure_prou_is_of_correct_order() {
        let one = make_scalar(1);
        let prou = Scalar::<Secp256k1>::from_bigint(
            &BigInt::from_str_radix(PRIMITIVE_ROOT_OF_UNITY, 10).unwrap(),
        );
        PowerIterator::new(prou, ROOT_OF_UNITY_BASIC_ORDER + 1)
            .enumerate()
            .for_each(|(exp, pow)| match exp {
                0 | ROOT_OF_UNITY_BASIC_ORDER => {
                    assert_eq!(one, pow);
                }
                _ => {
                    assert_ne!(one, pow);
                }
            });
    }
}
