use std::{collections::HashSet, fmt::Debug, sync::Arc};

use candle_core::{IndexOp, Result, Shape, Tensor, D};
use candle_nn::{Linear, Module, VarBuilder};
use loralinear::{LoraLinear, LoraLinearConfig};
use serde::Deserialize;

mod frozenlinear;
mod loralinear;

#[derive(Clone, Debug, Deserialize)]
pub struct LoraConfig {
    #[serde(rename = "r")]
    rank: usize,
    #[serde(rename = "lora_alpha")]
    alpha: f64,
    #[serde(rename = "lora_dropout")]
    dropout: Option<f32>,
    target_modules: HashSet<String>,
}

fn apply_scalings_to_x(
    x: Tensor,
    scalings_layer: &Tensor,
    adapter: usize,
    layer: usize,
) -> Result<Tensor> {
    let scalings = scalings_layer.i((.., .., adapter))?.unsqueeze(D::Minus1)?;
    if  layer == 60 {
        dbg!(&scalings);
        println!(
            "applied scalings {adapter}: {:?}",
            scalings.to_vec3::<half::bf16>().unwrap()
        );
        //let before = &x.to_vec3::<half::bf16>().unwrap()[0][0];
        //dbg!(before[0]);
    }
    let res = x.broadcast_mul(&scalings)?;
    //if adapter == 8 && layer == 60 {let after = &res.to_vec3::<half::bf16>().unwrap()[0][0];
    //dbg!(after[0]);}
    Ok(res)
}

impl LoraConfig {
    pub const fn new(
        rank: usize,
        alpha: f64,
        dropout: Option<f32>,
        target_modules: HashSet<String>,
    ) -> Self {
        Self {
            rank,
            alpha,
            dropout,
            target_modules,
        }
    }
}

/// Any layer that is linear-like.
pub trait LinearLayerLike: Debug {
    fn weight(&self) -> &Tensor;
    fn bias(&self) -> Option<&Tensor>;
    fn shape(&self) -> &Shape;
    fn lora_forward(&self, x: &Tensor, scalings_layer: Tensor) -> Result<Tensor>;
}

impl LinearLayerLike for Linear {
    fn weight(&self) -> &Tensor {
        self.weight()
    }
    fn bias(&self) -> Option<&Tensor> {
        self.bias()
    }
    fn shape(&self) -> &Shape {
        self.weight().shape()
    }
    fn lora_forward(&self, x: &Tensor, _scalings_layer: Tensor) -> Result<Tensor> {
        self.forward(x)
    }
}

pub fn linear(
    d1: usize,
    d2: usize,
    vb: VarBuilder,
    lora_config: &Vec<(String, LoraConfig)>,
    count: &mut usize,
) -> Result<Arc<dyn LinearLayerLike + Send + Sync>> {
    let prefix = vb.prefix();
    let module = prefix.split('.').last().unwrap();

    let linear_config = LoraLinearConfig::new(d1, d2);
    let inner = candle_nn::linear(d1, d2, vb.clone())?;

    let target_modules = &lora_config[0].1.target_modules;
    for (_, cfg) in lora_config {
        if &cfg.target_modules != target_modules {
            candle_core::bail!("Expected all target modules to be the same.");
        }
    }
    if !target_modules.contains(module) {
        return Ok(Arc::new(inner));
    }

    let lorainner = LoraLinear::new(&inner, &linear_config, lora_config, &vb, *count)?;
    *count += 1;
    Ok(Arc::new(lorainner))
}

pub fn linear_no_bias(
    d1: usize,
    d2: usize,
    vb: VarBuilder,
    lora_config: &Vec<(String, LoraConfig)>,
    count: &mut usize,
) -> Result<Arc<dyn LinearLayerLike + Send + Sync>> {
    let prefix = vb.prefix();
    let module = prefix.split('.').last().unwrap();

    let linear_config = LoraLinearConfig::new(d1, d2);
    let inner = candle_nn::linear_no_bias(d1, d2, vb.clone())?;

    let target_modules = &lora_config[0].1.target_modules;
    for (_, cfg) in lora_config {
        if &cfg.target_modules != target_modules {
            candle_core::bail!("Expected all target modules to be the same.");
        }
    }
    if !target_modules.contains(module) {
        return Ok(Arc::new(inner));
    }
    let lorainner = LoraLinear::new(&inner, &linear_config, lora_config, &vb, *count)?;
    *count += 1;
    Ok(Arc::new(lorainner))
}

fn get_maybe_topk_scalings(scalings: Tensor, layer: usize) -> Result<Tensor> {
    scalings.i((.., .., layer, ..))
}
