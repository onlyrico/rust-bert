// Copyright 2019-present, the HuggingFace Inc. team, The Google AI Language Team and Facebook, Inc.
// Copyright 2019-2020 Guillaume Becquin
// Copyright 2020 Maarten van Gompel
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//     http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//! # Sequence classification pipeline (e.g. Sentiment Analysis)
//! More generic sequence classification pipeline, works with multiple models (Bert, Roberta)
//!
//! ```no_run
//! use rust_bert::pipelines::sequence_classification::SequenceClassificationConfig;
//! use rust_bert::resources::{RemoteResource};
//! use rust_bert::distilbert::{DistilBertModelResources, DistilBertVocabResources, DistilBertConfigResources};
//! use rust_bert::pipelines::sequence_classification::SequenceClassificationModel;
//! use rust_bert::pipelines::common::ModelType;
//! # fn main() -> anyhow::Result<()> {
//!
//! //Load a configuration
//! let config = SequenceClassificationConfig::new(ModelType::DistilBert,
//!    RemoteResource::from_pretrained(DistilBertModelResources::DISTIL_BERT_SST2),
//!    RemoteResource::from_pretrained(DistilBertVocabResources::DISTIL_BERT_SST2),
//!    RemoteResource::from_pretrained(DistilBertConfigResources::DISTIL_BERT_SST2),
//!    None, // Merge resources
//!    true, //lowercase
//!    None, //strip_accents
//!    None, //add_prefix_space
//! );
//!
//! //Create the model
//! let sequence_classification_model = SequenceClassificationModel::new(config)?;
//!
//! let input = [
//!     "Probably my all-time favorite movie, a story of selflessness, sacrifice and dedication to a noble cause, but it's not preachy or boring.",
//!     "This film tried to be too many things all at once: stinging political satire, Hollywood blockbuster, sappy romantic comedy, family values promo...",
//!     "If you like original gut wrenching laughter you will like this movie. If you are young or old then you will love this movie, hell even my mom liked it.",
//! ];
//! let output = sequence_classification_model.predict(&input);
//! # Ok(())
//! # }
//! ```
//! (Example courtesy of [IMDb](http://www.imdb.com))
//!
//! Output: \
//! ```no_run
//! # use rust_bert::pipelines::sequence_classification::Label;
//! let output =
//! [
//!    Label { text: String::from("POSITIVE"), score: 0.9986, id: 1, sentence: 0},
//!    Label { text: String::from("NEGATIVE"), score: 0.9985, id: 0, sentence: 1},
//!    Label { text: String::from("POSITIVE"), score: 0.9988, id: 1, sentence: 12},
//! ]
//! # ;
//! ```
use crate::albert::AlbertForSequenceClassification;
use crate::bart::BartForSequenceClassification;
use crate::bert::BertForSequenceClassification;
use crate::common::error::RustBertError;
use crate::deberta::DebertaForSequenceClassification;
use crate::distilbert::DistilBertModelClassifier;
use crate::fnet::FNetForSequenceClassification;
use crate::longformer::LongformerForSequenceClassification;
use crate::mobilebert::MobileBertForSequenceClassification;
use crate::pipelines::common::{ConfigOption, ModelType, TokenizerOption};
use crate::reformer::ReformerForSequenceClassification;
use crate::resources::ResourceProvider;
use crate::roberta::RobertaForSequenceClassification;
use crate::xlnet::XLNetForSequenceClassification;
use rust_tokenizers::tokenizer::TruncationStrategy;
use rust_tokenizers::TokenizedInput;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashMap;
use tch::nn::VarStore;
use tch::{nn, no_grad, Device, Kind, Tensor};

use crate::deberta_v2::DebertaV2ForSequenceClassification;
#[cfg(feature = "remote")]
use crate::{
    distilbert::{DistilBertConfigResources, DistilBertModelResources, DistilBertVocabResources},
    resources::RemoteResource,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
/// # Label generated by a `SequenceClassificationModel`
pub struct Label {
    /// Label String representation
    pub text: String,
    /// Confidence score
    pub score: f64,
    /// Label ID
    pub id: i64,
    /// Sentence index
    #[serde(default)]
    pub sentence: usize,
}

/// # Configuration for SequenceClassificationModel
/// Contains information regarding the model to load and device to place the model on.
pub struct SequenceClassificationConfig {
    /// Model type
    pub model_type: ModelType,
    /// Model weights resource (default: pretrained BERT model on CoNLL)
    pub model_resource: Box<dyn ResourceProvider + Send>,
    /// Config resource (default: pretrained BERT model on CoNLL)
    pub config_resource: Box<dyn ResourceProvider + Send>,
    /// Vocab resource (default: pretrained BERT model on CoNLL)
    pub vocab_resource: Box<dyn ResourceProvider + Send>,
    /// Merges resource (default: None)
    pub merges_resource: Option<Box<dyn ResourceProvider + Send>>,
    /// Automatically lower case all input upon tokenization (assumes a lower-cased model)
    pub lower_case: bool,
    /// Flag indicating if the tokenizer should strip accents (normalization). Only used for BERT / ALBERT models
    pub strip_accents: Option<bool>,
    /// Flag indicating if the tokenizer should add a white space before each tokenized input (needed for some Roberta models)
    pub add_prefix_space: Option<bool>,
    /// Device to place the model on (default: CUDA/GPU when available)
    pub device: Device,
}

impl SequenceClassificationConfig {
    /// Instantiate a new sequence classification configuration of the supplied type.
    ///
    /// # Arguments
    ///
    /// * `model_type` - `ModelType` indicating the model type to load (must match with the actual data to be loaded!)
    /// * model - The `ResourceProvider` pointing to the model to load (e.g.  model.ot)
    /// * config - The `ResourceProvider` pointing to the model configuration to load (e.g. config.json)
    /// * vocab - The `ResourceProvider` pointing to the tokenizer's vocabulary to load (e.g.  vocab.txt/vocab.json)
    /// * vocab - An optional `ResourceProvider` pointing to the tokenizer's merge file to load (e.g.  merges.txt), needed only for Roberta.
    /// * lower_case - A `bool` indicating whether the tokenizer should lower case all input (in case of a lower-cased model)
    pub fn new<R>(
        model_type: ModelType,
        model_resource: R,
        config_resource: R,
        vocab_resource: R,
        merges_resource: Option<R>,
        lower_case: bool,
        strip_accents: impl Into<Option<bool>>,
        add_prefix_space: impl Into<Option<bool>>,
    ) -> SequenceClassificationConfig
    where
        R: ResourceProvider + Send + 'static,
    {
        SequenceClassificationConfig {
            model_type,
            model_resource: Box::new(model_resource),
            config_resource: Box::new(config_resource),
            vocab_resource: Box::new(vocab_resource),
            merges_resource: merges_resource.map(|r| Box::new(r) as Box<_>),
            lower_case,
            strip_accents: strip_accents.into(),
            add_prefix_space: add_prefix_space.into(),
            device: Device::cuda_if_available(),
        }
    }
}

#[cfg(feature = "remote")]
impl Default for SequenceClassificationConfig {
    /// Provides a defaultSST-2 sentiment analysis model (English)
    fn default() -> SequenceClassificationConfig {
        SequenceClassificationConfig::new(
            ModelType::DistilBert,
            RemoteResource::from_pretrained(DistilBertModelResources::DISTIL_BERT_SST2),
            RemoteResource::from_pretrained(DistilBertConfigResources::DISTIL_BERT_SST2),
            RemoteResource::from_pretrained(DistilBertVocabResources::DISTIL_BERT_SST2),
            None,
            true,
            None,
            None,
        )
    }
}

#[allow(clippy::large_enum_variant)]
/// # Abstraction that holds one particular sequence classification model, for any of the supported models
pub enum SequenceClassificationOption {
    /// Bert for Sequence Classification
    Bert(BertForSequenceClassification),
    /// DeBERTa for Sequence Classification
    Deberta(DebertaForSequenceClassification),
    /// DeBERTa V2 for Sequence Classification
    DebertaV2(DebertaV2ForSequenceClassification),
    /// DistilBert for Sequence Classification
    DistilBert(DistilBertModelClassifier),
    /// MobileBert for Sequence Classification
    MobileBert(MobileBertForSequenceClassification),
    /// Roberta for Sequence Classification
    Roberta(RobertaForSequenceClassification),
    /// XLMRoberta for Sequence Classification
    XLMRoberta(RobertaForSequenceClassification),
    /// Albert for Sequence Classification
    Albert(AlbertForSequenceClassification),
    /// XLNet for Sequence Classification
    XLNet(XLNetForSequenceClassification),
    /// Bart for Sequence Classification
    Bart(BartForSequenceClassification),
    /// Reformer for Sequence Classification
    Reformer(ReformerForSequenceClassification),
    /// Longformer for Sequence Classification
    Longformer(LongformerForSequenceClassification),
    /// FNet for Sequence Classification
    FNet(FNetForSequenceClassification),
}

impl SequenceClassificationOption {
    /// Instantiate a new sequence classification model of the supplied type.
    ///
    /// # Arguments
    ///
    /// * `model_type` - `ModelType` indicating the model type to load (must match with the actual data to be loaded)
    /// * `p` - `tch::nn::Path` path to the model file to load (e.g. model.ot)
    /// * `config` - A configuration (the model type of the configuration must be compatible with the value for
    /// `model_type`)
    pub fn new<'p, P>(
        model_type: ModelType,
        p: P,
        config: &ConfigOption,
    ) -> Result<Self, RustBertError>
    where
        P: Borrow<nn::Path<'p>>,
    {
        match model_type {
            ModelType::Bert => {
                if let ConfigOption::Bert(config) = config {
                    Ok(SequenceClassificationOption::Bert(
                        BertForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a BertConfig for Bert!".to_string(),
                    ))
                }
            }
            ModelType::Deberta => {
                if let ConfigOption::Deberta(config) = config {
                    Ok(SequenceClassificationOption::Deberta(
                        DebertaForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a DebertaConfig for DeBERTa!".to_string(),
                    ))
                }
            }
            ModelType::DebertaV2 => {
                if let ConfigOption::DebertaV2(config) = config {
                    Ok(SequenceClassificationOption::DebertaV2(
                        DebertaV2ForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a DebertaV2Config for DeBERTa V2!".to_string(),
                    ))
                }
            }
            ModelType::DistilBert => {
                if let ConfigOption::DistilBert(config) = config {
                    Ok(SequenceClassificationOption::DistilBert(
                        DistilBertModelClassifier::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a DistilBertConfig for DistilBert!".to_string(),
                    ))
                }
            }
            ModelType::MobileBert => {
                if let ConfigOption::MobileBert(config) = config {
                    Ok(SequenceClassificationOption::MobileBert(
                        MobileBertForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a MobileBertConfig for MobileBert!".to_string(),
                    ))
                }
            }
            ModelType::Roberta => {
                if let ConfigOption::Bert(config) = config {
                    Ok(SequenceClassificationOption::Roberta(
                        RobertaForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a BertConfig for Roberta!".to_string(),
                    ))
                }
            }
            ModelType::XLMRoberta => {
                if let ConfigOption::Bert(config) = config {
                    Ok(SequenceClassificationOption::XLMRoberta(
                        RobertaForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a BertConfig for Roberta!".to_string(),
                    ))
                }
            }
            ModelType::Albert => {
                if let ConfigOption::Albert(config) = config {
                    Ok(SequenceClassificationOption::Albert(
                        AlbertForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply an AlbertConfig for Albert!".to_string(),
                    ))
                }
            }
            ModelType::XLNet => {
                if let ConfigOption::XLNet(config) = config {
                    Ok(SequenceClassificationOption::XLNet(
                        XLNetForSequenceClassification::new(p, config).unwrap(),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply an XLNetConfig for XLNet!".to_string(),
                    ))
                }
            }
            ModelType::Bart => {
                if let ConfigOption::Bart(config) = config {
                    Ok(SequenceClassificationOption::Bart(
                        BartForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a BertConfig for Bert!".to_string(),
                    ))
                }
            }
            ModelType::Reformer => {
                if let ConfigOption::Reformer(config) = config {
                    Ok(SequenceClassificationOption::Reformer(
                        ReformerForSequenceClassification::new(p, config)?,
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a ReformerConfig for Reformer!".to_string(),
                    ))
                }
            }
            ModelType::Longformer => {
                if let ConfigOption::Longformer(config) = config {
                    Ok(SequenceClassificationOption::Longformer(
                        LongformerForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a LongformerConfig for Longformer!".to_string(),
                    ))
                }
            }
            ModelType::FNet => {
                if let ConfigOption::FNet(config) = config {
                    Ok(SequenceClassificationOption::FNet(
                        FNetForSequenceClassification::new(p, config),
                    ))
                } else {
                    Err(RustBertError::InvalidConfigurationError(
                        "You can only supply a FNetConfig for FNet!".to_string(),
                    ))
                }
            }
            _ => Err(RustBertError::InvalidConfigurationError(format!(
                "Sequence Classification not implemented for {:?}!",
                model_type
            ))),
        }
    }

    /// Returns the `ModelType` for this SequenceClassificationOption
    pub fn model_type(&self) -> ModelType {
        match *self {
            Self::Bert(_) => ModelType::Bert,
            Self::Deberta(_) => ModelType::Deberta,
            Self::DebertaV2(_) => ModelType::DebertaV2,
            Self::Roberta(_) => ModelType::Roberta,
            Self::XLMRoberta(_) => ModelType::Roberta,
            Self::DistilBert(_) => ModelType::DistilBert,
            Self::MobileBert(_) => ModelType::MobileBert,
            Self::Albert(_) => ModelType::Albert,
            Self::XLNet(_) => ModelType::XLNet,
            Self::Bart(_) => ModelType::Bart,
            Self::Reformer(_) => ModelType::Reformer,
            Self::Longformer(_) => ModelType::Longformer,
            Self::FNet(_) => ModelType::FNet,
        }
    }

    /// Interface method to forward_t() of the particular models.
    pub fn forward_t(
        &self,
        input_ids: Option<&Tensor>,
        mask: Option<&Tensor>,
        token_type_ids: Option<&Tensor>,
        position_ids: Option<&Tensor>,
        input_embeds: Option<&Tensor>,
        train: bool,
    ) -> Tensor {
        match *self {
            Self::Bart(ref model) => {
                model
                    .forward_t(
                        input_ids.expect("`input_ids` must be provided for BART models"),
                        mask,
                        None,
                        None,
                        None,
                        train,
                    )
                    .decoder_output
            }
            Self::Bert(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .logits
            }
            Self::Deberta(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .expect("Error in Deberta forward_t")
                    .logits
            }
            Self::DebertaV2(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .expect("Error in Deberta V2 forward_t")
                    .logits
            }
            Self::DistilBert(ref model) => {
                model
                    .forward_t(input_ids, mask, input_embeds, train)
                    .expect("Error in distilbert forward_t")
                    .logits
            }
            Self::MobileBert(ref model) => {
                model
                    .forward_t(input_ids, None, None, input_embeds, mask, train)
                    .expect("Error in mobilebert forward_t")
                    .logits
            }
            Self::Roberta(ref model) | Self::XLMRoberta(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .logits
            }
            Self::Albert(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .logits
            }
            Self::XLNet(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        None,
                        None,
                        None,
                        token_type_ids,
                        input_embeds,
                        train,
                    )
                    .logits
            }
            Self::Reformer(ref model) => {
                model
                    .forward_t(input_ids, None, None, mask, None, train)
                    .expect("Error in Reformer forward pass.")
                    .logits
            }
            Self::Longformer(ref model) => {
                model
                    .forward_t(
                        input_ids,
                        mask,
                        None,
                        token_type_ids,
                        position_ids,
                        input_embeds,
                        train,
                    )
                    .expect("Error in Longformer forward pass.")
                    .logits
            }
            Self::FNet(ref model) => {
                model
                    .forward_t(input_ids, token_type_ids, position_ids, input_embeds, train)
                    .expect("Error in FNet forward pass.")
                    .logits
            }
        }
    }
}

/// # SequenceClassificationModel for Classification (e.g. Sentiment Analysis)
pub struct SequenceClassificationModel {
    tokenizer: TokenizerOption,
    sequence_classifier: SequenceClassificationOption,
    label_mapping: HashMap<i64, String>,
    var_store: VarStore,
    max_length: usize,
}

impl SequenceClassificationModel {
    /// Build a new `SequenceClassificationModel`
    ///
    /// # Arguments
    ///
    /// * `config` - `SequenceClassificationConfig` object containing the resource references (model, vocabulary, configuration) and device placement (CPU/GPU)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> anyhow::Result<()> {
    /// use rust_bert::pipelines::sequence_classification::SequenceClassificationModel;
    ///
    /// let model = SequenceClassificationModel::new(Default::default())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        config: SequenceClassificationConfig,
    ) -> Result<SequenceClassificationModel, RustBertError> {
        let config_path = config.config_resource.get_local_path()?;
        let vocab_path = config.vocab_resource.get_local_path()?;
        let weights_path = config.model_resource.get_local_path()?;
        let merges_path = if let Some(merges_resource) = &config.merges_resource {
            Some(merges_resource.get_local_path()?)
        } else {
            None
        };
        let device = config.device;

        let tokenizer = TokenizerOption::from_file(
            config.model_type,
            vocab_path.to_str().unwrap(),
            merges_path.as_deref().map(|path| path.to_str().unwrap()),
            config.lower_case,
            config.strip_accents,
            config.add_prefix_space,
        )?;
        let mut var_store = VarStore::new(device);
        let model_config = ConfigOption::from_file(config.model_type, config_path);
        let max_length = model_config
            .get_max_len()
            .map(|v| v as usize)
            .unwrap_or(usize::MAX);
        let sequence_classifier =
            SequenceClassificationOption::new(config.model_type, &var_store.root(), &model_config)?;
        let label_mapping = model_config.get_label_mapping().clone();
        var_store.load(weights_path)?;
        Ok(SequenceClassificationModel {
            tokenizer,
            sequence_classifier,
            label_mapping,
            var_store,
            max_length,
        })
    }

    fn prepare_for_model<'a, S>(&self, input: S) -> Tensor
    where
        S: AsRef<[&'a str]>,
    {
        let tokenized_input: Vec<TokenizedInput> = self.tokenizer.encode_list(
            input.as_ref(),
            self.max_length,
            &TruncationStrategy::LongestFirst,
            0,
        );
        let max_len = tokenized_input
            .iter()
            .map(|input| input.token_ids.len())
            .max()
            .unwrap();
        let tokenized_input_tensors: Vec<tch::Tensor> = tokenized_input
            .iter()
            .map(|input| input.token_ids.clone())
            .map(|mut input| {
                input.extend(vec![
                    self.tokenizer.get_pad_id().expect(
                        "The Tokenizer used for sequence classification should contain a PAD id"
                    );
                    max_len - input.len()
                ]);
                input
            })
            .map(|input| Tensor::of_slice(&(input)))
            .collect::<Vec<_>>();
        Tensor::stack(tokenized_input_tensors.as_slice(), 0).to(self.var_store.device())
    }

    /// Classify texts
    ///
    /// # Arguments
    ///
    /// * `input` - `&[&str]` Array of texts to classify.
    ///
    /// # Returns
    ///
    /// * `Vec<Label>` containing labels for input texts
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> anyhow::Result<()> {
    /// # use rust_bert::pipelines::sequence_classification::SequenceClassificationModel;
    ///
    /// let sequence_classification_model =  SequenceClassificationModel::new(Default::default())?;
    /// let input = [
    ///     "Probably my all-time favorite movie, a story of selflessness, sacrifice and dedication to a noble cause, but it's not preachy or boring.",
    ///     "This film tried to be too many things all at once: stinging political satire, Hollywood blockbuster, sappy romantic comedy, family values promo...",
    ///     "If you like original gut wrenching laughter you will like this movie. If you are young or old then you will love this movie, hell even my mom liked it.",
    /// ];
    /// let output = sequence_classification_model.predict(&input);
    /// # Ok(())
    /// # }
    /// ```
    pub fn predict<'a, S>(&self, input: S) -> Vec<Label>
    where
        S: AsRef<[&'a str]>,
    {
        let input_tensor = self.prepare_for_model(input.as_ref());
        let output = no_grad(|| {
            let output = self.sequence_classifier.forward_t(
                Some(&input_tensor),
                None,
                None,
                None,
                None,
                false,
            );
            output.softmax(-1, Kind::Float).detach().to(Device::Cpu)
        });
        let label_indices = output.as_ref().argmax(-1, true).squeeze_dim(1);
        let scores = output
            .gather(1, &label_indices.unsqueeze(-1), false)
            .squeeze_dim(1);
        let label_indices = label_indices.iter::<i64>().unwrap().collect::<Vec<i64>>();
        let scores = scores.iter::<f64>().unwrap().collect::<Vec<f64>>();

        let mut labels: Vec<Label> = vec![];
        for sentence_idx in 0..label_indices.len() {
            let label_string = self
                .label_mapping
                .get(&label_indices[sentence_idx])
                .unwrap()
                .clone();
            let label = Label {
                text: label_string,
                score: scores[sentence_idx],
                id: label_indices[sentence_idx],
                sentence: sentence_idx,
            };
            labels.push(label)
        }
        labels
    }

    /// Multi-label classification of texts
    ///
    /// # Arguments
    ///
    /// * `input` - `&[&str]` Array of texts to classify.
    /// * `threshold` - `f64` threshold above which a label will be considered true by the classifier
    ///
    /// # Returns
    ///
    /// * `Vec<Vec<Label>>` containing a vector of true labels for each input text
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn main() -> anyhow::Result<()> {
    /// # use rust_bert::pipelines::sequence_classification::SequenceClassificationModel;
    ///
    /// let sequence_classification_model =  SequenceClassificationModel::new(Default::default())?;
    /// let input = [
    ///     "Probably my all-time favorite movie, a story of selflessness, sacrifice and dedication to a noble cause, but it's not preachy or boring.",
    ///     "This film tried to be too many things all at once: stinging political satire, Hollywood blockbuster, sappy romantic comedy, family values promo...",
    ///     "If you like original gut wrenching laughter you will like this movie. If you are young or old then you will love this movie, hell even my mom liked it.",
    /// ];
    /// let output = sequence_classification_model.predict_multilabel(&input, 0.5);
    /// # Ok(())
    /// # }
    /// ```
    pub fn predict_multilabel(
        &self,
        input: &[&str],
        threshold: f64,
    ) -> Result<Vec<Vec<Label>>, RustBertError> {
        let input_tensor = self.prepare_for_model(input);
        let output = no_grad(|| {
            let output = self.sequence_classifier.forward_t(
                Some(&input_tensor),
                None,
                None,
                None,
                None,
                false,
            );
            output.sigmoid().detach().to(Device::Cpu)
        });
        let label_indices = output.as_ref().ge(threshold).nonzero();

        let mut labels: Vec<Vec<Label>> = vec![];
        let mut sequence_labels: Vec<Label> = vec![];

        for sentence_idx in 0..label_indices.size()[0] {
            let label_index_tensor = label_indices.get(sentence_idx);
            let sentence_label = label_index_tensor
                .iter::<i64>()
                .unwrap()
                .collect::<Vec<i64>>();
            let (sentence, id) = (sentence_label[0], sentence_label[1]);
            if sentence as usize > labels.len() {
                labels.push(sequence_labels);
                sequence_labels = vec![];
            }
            let score = output.double_value(sentence_label.as_slice());
            let label_string = self.label_mapping.get(&id).unwrap().to_owned();
            let label = Label {
                text: label_string,
                score,
                id,
                sentence: sentence as usize,
            };
            sequence_labels.push(label);
        }
        if !sequence_labels.is_empty() {
            labels.push(sequence_labels);
        }
        Ok(labels)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[ignore] // no need to run, compilation is enough to verify it is Send
    fn test() {
        let config = SequenceClassificationConfig::default();
        let _: Box<dyn Send> = Box::new(SequenceClassificationModel::new(config));
    }
}
